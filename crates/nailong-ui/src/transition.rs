//! 双页合成转场引擎：彻底消除界面切换时的“跳一下”。
//!
//! 核心原理：
//! - 常规即时模式 UI 在切换页面那 1 帧，旧页面瞬间消失、新页面凭空出现，人眼会看到一次生硬闪断。
//! - 本模块直接在 `egui::Context::graphics` 的 `PaintList` 上抓取上一帧已画好的矢量图形快照
//!   （`TextShape` 只共享 `Arc<Galley>`，平移与透明度调节零深拷贝、零堆分配）：
//!   1. **页面与子区域切换（`run_region`）**：旧画面留在屏幕上继续向反方向柔和滑出并淡出，
//!      同时新画面在最终布局矩形内原位排版（零布局抖动、零重排）后，在绘制层向语义方向同步滑入并淡入；
//!      第 0 帧（`t = 0`）旧画面 100% 原位、新画面 0% 透明，与切换前完全连续，数学上消除跳变。
//!   2. **弹窗开合（`modal_enter` / `finish_modal` / `paint_closing_modals`）**：
//!      不仅打开时对整个弹窗框架（含外框底色、边框、投影与全部内容）统一淡入上浮，
//!      关闭时也会自动用最后一帧快照柔和淡出回落，按 `Esc` 不再生硬消失。
//! - 过渡期间自动跟随屏幕原生刷新率（60/120/144/240Hz），结束后立即停止请求重绘（空闲 CPU 仍为 0%）；
//!   开启「减少动效」时所有过渡直接零延迟到位。

use crate::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// 区域快照与进行中的双页合成转场。
#[derive(Clone)]
struct RegionMem {
    last_frame: u64,
    last_key: u64,
    last_shapes: Arc<Vec<egui::epaint::ClippedShape>>,
    last_rect: egui::Rect,
    active: Option<ActiveRegion>,
}

#[derive(Clone)]
struct ActiveRegion {
    outgoing: Arc<Vec<egui::epaint::ClippedShape>>,
    started_at: f64,
    duration: f64,
    dir: egui::Vec2,
    enter_dist: f32,
    exit_dist: f32,
}

/// 弹窗开合配置与规格。
#[derive(Clone, Copy)]
pub(crate) struct Spec {
    pub(crate) duration: f64,
    pub(crate) enter_dist: f32,
    pub(crate) exit_dist: f32,
}

impl Spec {
    /// 一级侧栏切页（纵向耦合滑行，柔和从容）。
    pub(crate) const PAGE_VERTICAL: Self = Self {
        duration: 0.26,
        enter_dist: 34.0,
        exit_dist: 22.0,
    };

    /// 模块内钻取与返回（如 版本↔安装、已安装↔发现↔详情，横向推拉）。
    pub(crate) const PAGE_HORIZONTAL: Self = Self {
        duration: 0.27,
        enter_dist: 46.0,
        exit_dist: 26.0,
    };

    /// 页内右侧详情或子分栏切换（轻巧柔和，不喧宾夺主）。
    pub(crate) const SUB_PANE: Self = Self {
        duration: 0.20,
        enter_dist: 18.0,
        exit_dist: 12.0,
    };

    /// 页内横向子标签切换（Mod/资源包/光影、截图总览/时间/地点/人物）。
    pub(crate) const SUB_TAB: Self = Self {
        duration: 0.21,
        enter_dist: 24.0,
        exit_dist: 14.0,
    };
}

/// 在给定 `stage_rect` 内运行一次双页合成转场区域。
///
/// - `key`：当前内容标识；变化时自动用上一帧图形作为退出层，与本帧新内容同步交叉淡入滑行。
/// - `dir_spec`：根据 `(old_key, new_key)` 返回 `(滑动方向单位向量, Spec)`。
/// - 首次出现或父页面刚切入时自动跳过子转场，绝不与外层切页叠加重影。
pub(crate) fn run_region<F>(
    ui: &mut egui::Ui,
    id: egui::Id,
    key: u64,
    stage_rect: egui::Rect,
    dir_spec: impl FnOnce(u64, u64) -> (egui::Vec2, Spec),
    render: F,
) where
    F: FnOnce(&mut egui::Ui),
{
    let ctx = ui.ctx().clone();
    let layer_id = ui.layer_id();
    let now = ctx.input(|i| i.time);
    let frame = ctx.cumulative_frame_nr();
    let key_hash = key;

    let mut mem = ctx.data(|d| d.get_temp::<RegionMem>(id));
    let just_appeared = match &mem {
        Some(m) => frame > m.last_frame.saturating_add(2),
        None => true,
    };

    if just_appeared || motion::reduced() {
        if let Some(m) = &mut mem {
            m.active = None;
        }
    } else if let Some(m) = &mut mem
        && m.last_key != key_hash
    {
        let size_stable = (m.last_rect.size() - stage_rect.size()).length() < 48.0;
        if size_stable && !m.last_shapes.is_empty() {
            let (dir, spec) = dir_spec(m.last_key, key_hash);
            m.active = Some(ActiveRegion {
                outgoing: Arc::clone(&m.last_shapes),
                started_at: now,
                duration: spec.duration,
                dir,
                enter_dist: spec.enter_dist,
                exit_dist: spec.exit_dist,
            });
        } else {
            m.active = None;
        }
    }

    // 计算当前转场帧参数；已结束则立即清掉并停止重绘。
    let active_params = mem.as_mut().and_then(|m| {
        let tr = m.active.as_ref()?;
        let raw_t = ((now - tr.started_at) / tr.duration) as f32;
        if raw_t >= 1.0 || motion::reduced() {
            m.active = None;
            None
        } else {
            let t = raw_t.clamp(0.0, 1.0);
            ctx.request_repaint();
            let enter_pos = motion::ease_out_cubic(t);
            let exit_pos = motion::ease_out_cubic(t);
            let enter_offset = tr.dir * (tr.enter_dist * (1.0 - enter_pos));
            let exit_offset = -tr.dir * (tr.exit_dist * exit_pos);
            // 错峰交叉淡变（Shared-Axis）：旧页在前 44% 迅速清空，新页从 14% 柔和显现到 76% 满透，
            // 避免两页文字大面积重影打架，同时保持全程亮度连贯无黑屏闪断。
            let exit_alpha = 1.0 - motion::ease_in_out_cubic((t / 0.44).clamp(0.0, 1.0));
            let enter_alpha = motion::ease_out_cubic(((t - 0.12) / 0.64).clamp(0.0, 1.0));
            Some((
                Arc::clone(&tr.outgoing),
                exit_offset,
                exit_alpha,
                enter_offset,
                enter_alpha,
            ))
        }
    });

    // 1) 若处于转场中，先把退出页面画在底层（平移 + 淡出 + 严格裁切在 stage_rect 内）。
    if let Some((ref outgoing, exit_offset, exit_alpha, _, _)) = active_params
        && exit_alpha > 0.005
    {
        ctx.graphics_mut(|g| {
            let list = g.entry(layer_id);
            for orig in outgoing.iter() {
                let clipped = orig.clip_rect.translate(exit_offset).intersect(stage_rect);
                if clipped.is_positive() {
                    let mut shape = orig.shape.clone();
                    translate_shape_fast(&mut shape, exit_offset);
                    fade_shape_fast(&mut shape, exit_alpha);
                    list.add(clipped, shape);
                }
            }
        });
    }

    // 2) 新页面始终在最终的 stage_rect 原位排版，彻底避免布局跳动或面板重排。
    let enter_start = ctx.graphics(|g| g.get(layer_id).map_or(0, |l| l.next_idx().0));
    render(ui);
    let enter_end = ctx.graphics(|g| g.get(layer_id).map_or(0, |l| l.next_idx().0));

    // 3) 若处于转场中，对刚画出的新页面图形原地施加滑入偏移与统一淡入。
    if let Some((_, _, _, enter_offset, enter_alpha)) = active_params {
        ctx.graphics_mut(|g| {
            if let Some(list) = g.get_mut(layer_id) {
                for i in enter_start..enter_end {
                    list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                        cs.clip_rect = cs.clip_rect.translate(enter_offset).intersect(stage_rect);
                        if enter_alpha <= 0.004 || !cs.clip_rect.is_positive() {
                            cs.shape = egui::Shape::Noop;
                        } else {
                            translate_shape_fast(&mut cs.shape, enter_offset);
                            fade_shape_fast(&mut cs.shape, enter_alpha);
                        }
                    });
                }
            }
        });
    }

    // 4) 记录当前可见的新画面快照，供下一次切换时平滑接续。
    //    在极早期（第 0 帧 enter_alpha 接近 0）保留上一帧有效快照，防止极速双击抓到空白帧。
    let should_snapshot = match active_params {
        None => true,
        Some((_, _, _, _, enter_alpha)) => enter_alpha > 0.35,
    };
    let next_shapes = if should_snapshot {
        let captured = ctx.graphics(|g| {
            g.get(layer_id)
                .map(|list| {
                    list.all_entries()
                        .skip(enter_start)
                        .take(enter_end.saturating_sub(enter_start))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
        Arc::new(captured)
    } else {
        mem.as_ref()
            .map(|m| Arc::clone(&m.last_shapes))
            .unwrap_or_default()
    };

    let active_keep = mem.and_then(|m| m.active);
    ctx.data_mut(|d| {
        d.insert_temp(
            id,
            RegionMem {
                last_frame: frame,
                last_key: key_hash,
                last_shapes: next_shapes,
                last_rect: stage_rect,
                active: active_keep,
            },
        );
    });
}

// ============================================================================
// 弹窗开合统一转场（打开淡入上浮 + 关闭平滑淡出回落）
// ============================================================================

#[derive(Clone)]
struct ModalEntry {
    last_frame: u64,
    opened_at: f64,
    closing_at: Option<f64>,
    veil_scale: f32,
    shapes: Arc<Vec<egui::epaint::ClippedShape>>,
}

#[derive(Clone, Default)]
struct ModalRegistry {
    entries: HashMap<egui::Id, ModalEntry>,
}

fn modal_registry_id() -> egui::Id {
    egui::Id::unique("cairn_modal_transition_registry")
}

/// 获取弹窗打开进度（0..=1，用于设定遮罩透明度）。
/// 第 0 帧返回 0.0，配合 `finish_modal` 实现丝滑连续的打开过程。
pub(crate) fn modal_enter(ctx: &egui::Context, modal_id: egui::Id) -> f32 {
    if motion::reduced() {
        return 1.0;
    }
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|i| i.time);
    let mut reg = ctx
        .data(|d| d.get_temp::<ModalRegistry>(modal_registry_id()))
        .unwrap_or_default();
    let opened_at = match reg.entries.get(&modal_id) {
        Some(entry)
            if entry.closing_at.is_none() && frame <= entry.last_frame.saturating_add(2) =>
        {
            entry.opened_at
        }
        _ => now,
    };
    let entry = reg.entries.entry(modal_id).or_insert_with(|| ModalEntry {
        last_frame: frame,
        opened_at,
        closing_at: None,
        veil_scale: 1.0,
        shapes: Arc::new(Vec::new()),
    });
    entry.opened_at = opened_at;
    entry.closing_at = None;
    entry.last_frame = frame;
    ctx.data_mut(|d| d.insert_temp(modal_registry_id(), reg));

    let raw = (((now - opened_at) / 0.20) as f32).clamp(0.0, 1.0);
    if raw < 1.0 {
        ctx.request_repaint();
    }
    motion::ease_out_cubic(raw)
}

/// 在 `egui::Modal::show`（或 `egui::Area::show`）之后调用：
/// - 保存未淡化前的卡片图形快照，供关闭时平滑淡出；
/// - 打开阶段对整张弹窗卡片（含外框、细边、投影和全部子控件）统一施加柔和上浮与淡入。
///
/// `has_backdrop`: `egui::Modal` 传 `true`（跳过索引 0 的全屏遮罩矩形），`egui::Area` 浮层传 `false`。
pub(crate) fn finish_modal(
    ctx: &egui::Context,
    modal_id: egui::Id,
    layer_id: egui::LayerId,
    enter: f32,
    veil_scale: f32,
    has_backdrop: bool,
) {
    if motion::reduced() {
        return;
    }
    let skip = usize::from(has_backdrop);
    let snapshot: Vec<egui::epaint::ClippedShape> = ctx.graphics(|g| {
        g.get(layer_id)
            .map(|list| list.all_entries().skip(skip).cloned().collect())
            .unwrap_or_default()
    });
    let frame = ctx.cumulative_frame_nr();
    let mut reg = ctx
        .data(|d| d.get_temp::<ModalRegistry>(modal_registry_id()))
        .unwrap_or_default();
    if let Some(entry) = reg.entries.get_mut(&modal_id) {
        entry.last_frame = frame;
        entry.veil_scale = if has_backdrop { veil_scale } else { 0.0 };
        if !snapshot.is_empty() {
            entry.shapes = Arc::new(snapshot);
        }
    }
    ctx.data_mut(|d| d.insert_temp(modal_registry_id(), reg));

    if enter < 0.999 {
        let slide = egui::vec2(0.0, (1.0 - enter) * 14.0);
        ctx.graphics_mut(|g| {
            if let Some(list) = g.get_mut(layer_id) {
                let end = list.next_idx().0;
                for i in skip..end {
                    list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                        cs.clip_rect = cs.clip_rect.translate(slide);
                        if enter <= 0.004 {
                            cs.shape = egui::Shape::Noop;
                        } else {
                            translate_shape_fast(&mut cs.shape, slide);
                            fade_shape_fast(&mut cs.shape, enter);
                        }
                    });
                }
            }
        });
    }
}

/// 每帧在全部弹窗绘制完毕后调用一次：自动为刚关闭的弹窗播放 140ms 柔和淡出回落动效。
pub(crate) fn paint_closing_modals(ctx: &egui::Context) {
    if motion::reduced() {
        return;
    }
    let mut reg = ctx
        .data(|d| d.get_temp::<ModalRegistry>(modal_registry_id()))
        .unwrap_or_default();
    if reg.entries.is_empty() {
        return;
    }
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|i| i.time);
    let screen = ctx.content_rect();
    let ghost_layer = egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("cairn_modal_closing_ghost"),
    );

    let mut remove = Vec::new();
    let mut need_repaint = false;

    for (&id, entry) in &mut reg.entries {
        if entry.last_frame == frame {
            continue;
        }
        // 刚刚关闭：这一帧没有画，但上一帧还在屏幕上。
        let closing_at = match entry.closing_at {
            Some(at) => at,
            None if frame <= entry.last_frame.saturating_add(2) && !entry.shapes.is_empty() => {
                entry.closing_at = Some(now);
                now
            }
            None => {
                remove.push(id);
                continue;
            }
        };
        let raw = ((now - closing_at) / 0.17) as f32;
        if raw >= 1.0 {
            remove.push(id);
            continue;
        }
        need_repaint = true;
        let eased = motion::ease_out_cubic(raw.clamp(0.0, 1.0));
        let alpha = (1.0 - eased).clamp(0.0, 1.0);
        let slide = egui::vec2(0.0, eased * 10.0);

        ctx.graphics_mut(|g| {
            let list = g.entry(ghost_layer);
            if entry.veil_scale > 0.0 && alpha > 0.005 {
                list.add(
                    screen,
                    egui::Shape::rect_filled(screen, 0.0, t::veil(entry.veil_scale * alpha)),
                );
            }
            if alpha > 0.005 {
                for orig in entry.shapes.iter() {
                    let clipped = orig.clip_rect.translate(slide).intersect(screen);
                    if clipped.is_positive() {
                        let mut shape = orig.shape.clone();
                        translate_shape_fast(&mut shape, slide);
                        fade_shape_fast(&mut shape, alpha);
                        list.add(clipped, shape);
                    }
                }
            }
        });
    }

    for id in remove {
        reg.entries.remove(&id);
    }
    if need_repaint {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(modal_registry_id(), reg));
}

// ============================================================================
// 全屏深浅主题平滑交融（Dark ↔ Light 零闪断亮度柔化过渡）
// ============================================================================

#[derive(Clone, Copy, Default)]
struct ThemeDissolve {
    old_bg: egui::Color32,
    peak_alpha: f32,
    started_at: f64,
    duration: f64,
}

fn theme_dissolve_id() -> egui::Id {
    egui::Id::unique("cairn_theme_dissolve")
}

fn theme_dissolve_layer() -> egui::LayerId {
    egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("cairn_theme_dissolve_layer"),
    )
}

/// 切换深浅外观时启动 220ms 哑光亮度交融：按人眼亮度曲线设定起始不透明度
/// （深→浅 0.85 单调提亮防闪白；浅→深 0.55 单调沉降防泛白），绝不触碰跨主题字体图集。
pub(crate) fn begin_theme_dissolve(ctx: &egui::Context, old_bg: egui::Color32) {
    if motion::reduced() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let old_is_light = old_bg.r() > 128;
    ctx.data_mut(|d| {
        d.insert_temp(
            theme_dissolve_id(),
            ThemeDissolve {
                old_bg,
                peak_alpha: if old_is_light { 0.55 } else { 0.85 },
                started_at: now,
                duration: 0.22,
            },
        );
    });
    ctx.request_repaint();
}

/// 每帧收尾时绘制深浅主题亮度交融层；220ms 后自动清除并停止重绘（空闲 CPU 为 0%）。
pub(crate) fn paint_theme_dissolve(ctx: &egui::Context) {
    let Some(tr) = ctx.data(|d| d.get_temp::<ThemeDissolve>(theme_dissolve_id())) else {
        return;
    };
    let now = ctx.input(|i| i.time);
    let raw = ((now - tr.started_at) / tr.duration) as f32;
    if raw >= 1.0 || motion::reduced() {
        ctx.data_mut(|d| d.remove_temp::<ThemeDissolve>(theme_dissolve_id()));
        return;
    }
    ctx.request_repaint();
    let alpha =
        ((1.0 - motion::ease_out_cubic(raw.clamp(0.0, 1.0))) * tr.peak_alpha).clamp(0.0, 1.0);
    if alpha <= 0.005 {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(theme_dissolve_layer());
    painter.rect_filled(screen, 0.0, tr.old_bg.gamma_multiply(alpha));
}

// ============================================================================
// 零深拷贝图形平移与透明度调节
// ============================================================================

/// 对任意 `egui::Shape` 原地平移；文字只改 `pos`，不克隆 `Arc<Galley>`。
pub(crate) fn translate_shape_fast(shape: &mut egui::Shape, delta: egui::Vec2) {
    if delta == egui::Vec2::ZERO {
        return;
    }
    match shape {
        egui::Shape::Noop => {}
        egui::Shape::Text(text) => {
            text.pos += delta;
        }
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                translate_shape_fast(s, delta);
            }
        }
        other => other.translate(delta),
    }
}

/// 对任意 `egui::Shape` 原地乘以透明度；文字直接走 GPU `opacity_factor`，零 `Galley` 深拷贝。
pub(crate) fn fade_shape_fast(shape: &mut egui::Shape, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity >= 0.999 {
        return;
    }
    if opacity <= 0.004 {
        *shape = egui::Shape::Noop;
        return;
    }
    match shape {
        egui::Shape::Noop => {}
        egui::Shape::Text(text) => {
            text.opacity_factor *= opacity;
        }
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                fade_shape_fast(s, opacity);
            }
        }
        other => {
            egui::epaint::shape_transform::adjust_colors(other, move |color| {
                if *color != egui::Color32::PLACEHOLDER {
                    *color = color.gamma_multiply(opacity);
                }
            });
        }
    }
}

pub(crate) fn hash_u64(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
