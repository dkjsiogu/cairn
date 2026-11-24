//! 基础控件和动效工具。
//!
//! 过渡都很短，结束后不再请求重绘，所以空闲时 CPU 是 0。

use eframe::egui;
use egui::{Color32, Rect, Sense, Shape, Stroke, pos2, vec2};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::theme as t;

/// 统一圆角刻度：基础控件 8、卡片/缩略图 12、英雄舞台 16、弹窗 14。
pub const R: u8 = 8;
pub const R_CARD: u8 = 12;
pub const R_HERO: u8 = 16;
pub const R_MODAL: u8 = 14;

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 苹果风格的柔顺减速曲线：起手轻快、落点丝滑。
pub fn ease_out_quart(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(4)
}

/// 临界阻尼弹簧矩形：让侧栏选中光带、分段胶囊、下划线和命令面板光标在各项之间丝滑滑行，
/// 到位后立即停止重绘（空闲 CPU 仍为 0%）。
pub fn spring_rect(ctx: &egui::Context, id: egui::Id, target: Rect) -> Rect {
    if reduced() {
        return target;
    }
    let raw_dt = ctx.input(|input| input.stable_dt);
    let stored = ctx.data(|d| d.get_temp::<([f32; 4], [f32; 4])>(id));
    let goal = [target.min.x, target.min.y, target.max.x, target.max.y];
    let Some((mut x, mut v)) = stored else {
        ctx.data_mut(|d| d.insert_temp(id, (goal, [0.0; 4])));
        return target;
    };
    // 目标突变太大（例如窗口大幅缩放或从窄栏切宽栏）时避免横跨半屏，直接贴合。
    let jump = (goal[0] - x[0]).abs().max((goal[1] - x[1]).abs());
    if jump > 520.0 {
        ctx.data_mut(|d| d.insert_temp(id, (goal, [0.0; 4])));
        return target;
    }
    // 从静息态刚被唤醒的首帧把 dt 限在 1/60s，避免空闲后第一帧猛跳一大截。
    let from_rest = v.iter().all(|vel| vel.abs() < 1e-3);
    let dt = if from_rest {
        raw_dt.min(1.0 / 60.0)
    } else {
        raw_dt.min(1.0 / 20.0)
    };
    let stiffness = 380.0_f32;
    let damping = 36.0_f32;
    let steps = 4;
    let h = dt / steps as f32;
    let mut moving = false;
    for i in 0..4 {
        for _ in 0..steps {
            v[i] += (stiffness * (goal[i] - x[i]) - damping * v[i]) * h;
            x[i] += v[i] * h;
        }
        if (x[i] - goal[i]).abs() < 0.25 && v[i].abs() < 1.5 {
            x[i] = goal[i];
            v[i] = 0.0;
        } else {
            moving = true;
        }
    }
    if moving {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, (x, v)));
    Rect::from_min_max(pos2(x[0], x[1]), pos2(x[2], x[3]))
}

/// 弹窗打开时的自动入场进度（0..=1）：通过帧号自动识别「刚打开」，无需各弹窗手写计时器。
pub fn modal_enter(ctx: &egui::Context, id: egui::Id) -> f32 {
    if reduced() {
        return 1.0;
    }
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|i| i.time);
    let key = id.with("cairn_modal_enter");
    let (last_frame, opened_at) = ctx
        .data(|d| d.get_temp::<(u64, f64)>(key))
        .unwrap_or((0, now));
    let opened_at = if last_frame == 0 || frame > last_frame.saturating_add(2) {
        now
    } else {
        opened_at
    };
    ctx.data_mut(|d| d.insert_temp(key, (frame, opened_at)));
    let raw = (((now - opened_at) / 0.19) as f32).clamp(0.0, 1.0);
    if raw < 1.0 {
        ctx.request_repaint();
    }
    ease_out_quart(raw)
}

/// 弹窗打开时的柔和上浮位移（px）。
pub fn modal_slide_y(eased: f32) -> f32 {
    (1.0 - eased.clamp(0.0, 1.0)) * 12.0
}

/// 页内子标签或左侧列表切换时的柔和淡入（0..=1）。
/// 刚切进当前页面时不重复淡入（交给整页转场），只在同页内切换选项时触发。
pub fn key_fade(
    ctx: &egui::Context,
    id: egui::Id,
    key: impl std::hash::Hash,
    duration: f64,
) -> f32 {
    if reduced() {
        return 1.0;
    }
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    let hash = hasher.finish();
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|i| i.time);
    let prev = ctx.data(|d| d.get_temp::<(u64, u64, f64)>(id));
    let changed_at = match prev {
        Some((last_frame, last_hash, changed_at)) if frame <= last_frame.saturating_add(2) => {
            if last_hash != hash {
                now
            } else {
                changed_at
            }
        }
        _ => now - duration - 1.0,
    };
    ctx.data_mut(|d| d.insert_temp(id, (frame, hash, changed_at)));
    let raw = (((now - changed_at) / duration) as f32).clamp(0.0, 1.0);
    if raw < 1.0 {
        ctx.request_repaint();
    }
    ease_out_quart(raw)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| egui::lerp(x as f32..=y as f32, t).round() as u8;
    Color32::from_rgba_premultiplied(
        ch(a.r(), b.r()),
        ch(a.g(), b.g()),
        ch(a.b(), b.b()),
        ch(a.a(), b.a()),
    )
}

/// 欠阻尼弹簧，只用在状态圆环上。运动中才请求下一帧。
pub fn spring(ctx: &egui::Context, id: egui::Id, target: f32, stiffness: f32, damping: f32) -> f32 {
    if reduced() {
        return target;
    }
    let dt = ctx.input(|input| input.stable_dt).min(1.0 / 20.0);
    let (mut x, mut v) = ctx
        .data(|data| data.get_temp::<(f32, f32)>(id))
        .unwrap_or((target, 0.0));
    let steps = 4;
    let h = dt / steps as f32;
    for _ in 0..steps {
        v += (stiffness * (target - x) - damping * v) * h;
        x += v * h;
    }
    if (x - target).abs() < 0.002 && v.abs() < 0.01 {
        x = target;
        v = 0.0;
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|data| data.insert_temp(id, (x, v)));
    x
}

static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn set_reduced(value: bool) {
    REDUCED.store(value, Ordering::Relaxed);
}

pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// 悬停过渡的进度；减少动效时直接到位。
pub fn hover_amount(ui: &egui::Ui, id: egui::Id, hovered: bool) -> f32 {
    if reduced() {
        return if hovered { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time(id, hovered, 0.12)
}

/// 指着能点的东西时变成手形。自己画的行、卡片、图标按钮不用各自设置：每帧最后看一眼
/// 指针下面的控件，能点、没禁用、又不是拖动用的，就换成手形。已经设了别的光标
/// （输入框的竖线、拖动时的手掌）就不动。
pub fn pointer_on_clickables(ctx: &egui::Context) {
    if ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default {
        return;
    }
    let hovered: Vec<egui::Id> = ctx.interaction_snapshot(|s| s.hovered.iter().copied().collect());
    let clickable = hovered.into_iter().any(|id| {
        ctx.read_response(id).is_some_and(|response| {
            response.enabled() && response.sense.senses_click() && !response.sense.senses_drag()
        })
    });
    if clickable {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// 标记自带聚焦边框的输入框，避免再套一层外部焦点框。
pub fn mark_custom_focus(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.insert_temp(id.with("cairn_custom_focus"), true));
}

/// 键盘聚焦的外框。每帧统一画一次，所有控件都能用上。
pub fn focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|memory| memory.focused()) else {
        return;
    };
    if ctx.data(|d| {
        d.get_temp::<bool>(id.with("cairn_custom_focus"))
            .unwrap_or(false)
    }) {
        return;
    }
    let Some(response) = ctx.read_response(id) else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("nailong_focus_ring"),
    ));
    painter.rect_stroke(
        response.rect.expand(2.0),
        R + 2,
        Stroke::new(2.0, t::accent()),
        egui::StrokeKind::Outside,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrimaryGlyph {
    None,
    Play,
    Download,
}

pub fn primary_sized(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    width: f32,
    height: f32,
    font_size: f32,
) -> bool {
    primary_sized_glyph(
        ui,
        text,
        enabled,
        width,
        height,
        font_size,
        PrimaryGlyph::None,
    )
}

/// 主按钮：立体渐变底色 + 底部色光环境阴影 + 顶部 1px 玻璃高光。
pub fn primary_sized_glyph(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    width: f32,
    height: f32,
    _font_size: f32,
    glyph: PrimaryGlyph,
) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width.max(72.0), height.max(32.0)), sense);
    let hover = hover_amount(ui, response.id.with("hover"), enabled && response.hovered());
    let pressed = enabled && response.is_pointer_button_down_on();
    let lift_y = if !enabled || reduced() {
        0.0
    } else if pressed {
        1.0
    } else {
        -hover
    };
    let draw = rect.translate(vec2(0.0, lift_y));
    let color = if !enabled { t::dim() } else { t::on_accent() };
    if enabled {
        if !pressed {
            ui.painter()
                .add(t::glow_shadow(t::accent(), 0.78 + 0.50 * hover).as_shape(draw, R));
        }
        crate::backdrop::paint_primary_fill(ui.ctx(), ui.painter(), draw, R, hover, pressed);
        let top_highlight = Color32::from_white_alpha(if t::is_light() { 96 } else { 78 });
        let rim_x0 = draw.left() + R as f32;
        let rim_x1 = draw.right() - R as f32;
        if rim_x1 > rim_x0 && !pressed {
            ui.painter().line_segment(
                [
                    pos2(rim_x0, draw.top() + 1.0),
                    pos2(rim_x1, draw.top() + 1.0),
                ],
                Stroke::new(1.0, top_highlight),
            );
        }
        let edge = if pressed {
            t::accent_press()
        } else {
            mix(t::tint(t::accent_top(), 0.78), t::accent_hover_top(), hover)
        };
        ui.painter()
            .rect_stroke(draw, R, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    } else {
        ui.painter().rect(
            draw,
            R,
            t::wash(),
            Stroke::new(1.0, t::hairline()),
            egui::StrokeKind::Inside,
        );
    }
    paint_label(ui.painter(), draw, text, glyph, color, 14.0, true);
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

fn paint_label(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    glyph: PrimaryGlyph,
    color: Color32,
    font_px: f32,
    bold: bool,
) {
    let family = if bold {
        crate::bold_family()
    } else {
        egui::FontFamily::Proportional
    };
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::new(font_px, family), color);
    let icon_w = if glyph == PrimaryGlyph::None {
        0.0
    } else {
        14.0
    };
    let gap = if glyph == PrimaryGlyph::None {
        0.0
    } else {
        8.0
    };
    let left = rect.center().x - (icon_w + gap + galley.size().x) * 0.5;
    if glyph != PrimaryGlyph::None {
        paint_glyph(
            painter,
            pos2(left + icon_w * 0.5, rect.center().y),
            glyph,
            color,
        );
    }
    painter.galley(
        pos2(left + icon_w + gap, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

fn paint_glyph(painter: &egui::Painter, c: egui::Pos2, glyph: PrimaryGlyph, color: Color32) {
    match glyph {
        PrimaryGlyph::None => {}
        PrimaryGlyph::Play => {
            painter.add(Shape::convex_polygon(
                vec![
                    c + vec2(-4.0, -6.0),
                    c + vec2(6.0, 0.0),
                    c + vec2(-4.0, 6.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        PrimaryGlyph::Download => {
            let stroke = Stroke::new(1.6, color);
            painter.line_segment([c + vec2(0.0, -6.0), c + vec2(0.0, 2.0)], stroke);
            painter.add(Shape::line(
                vec![
                    c + vec2(-4.0, -2.0),
                    c + vec2(0.0, 2.5),
                    c + vec2(4.0, -2.0),
                ],
                stroke,
            ));
            painter.line_segment([c + vec2(-6.0, 6.0), c + vec2(6.0, 6.0)], stroke);
        }
    }
}

/// 次按钮：玻璃感轻表面 + 顶部微高光 + 平滑悬停反馈。
pub fn secondary_sized(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    width: f32,
    height: f32,
    _font_size: f32,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width.max(48.0), height.max(32.0)), sense);
    let hover = hover_amount(ui, response.id.with("hover"), enabled && response.hovered());
    let pressed = enabled && response.is_pointer_button_down_on();
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if pressed {
        mix(t::wash(), t::control(), 0.25)
    } else {
        mix(t::surface_glass(), t::elevated(), hover)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, R, fill);
    }
    if enabled && !pressed && rect.width() > R as f32 * 2.0 {
        ui.painter().line_segment(
            [
                pos2(rect.left() + R as f32, rect.top() + 1.0),
                pos2(rect.right() - R as f32, rect.top() + 1.0),
            ],
            Stroke::new(1.0, t::rim()),
        );
    }
    ui.painter().rect_stroke(
        rect,
        R,
        Stroke::new(1.0, mix(t::glass_border(), t::control(), hover)),
        egui::StrokeKind::Inside,
    );
    let color = if enabled { t::ink() } else { t::dim() };
    paint_label(
        ui.painter(),
        rect,
        text,
        PrimaryGlyph::None,
        color,
        13.5,
        false,
    );
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// 高 7 的流光渐变进度条。
pub fn progress_bar(ui: &mut egui::Ui, fraction: f32, indeterminate: bool, time: f64) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 7.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        4,
        t::rail(),
        Stroke::new(1.0, t::hairline()),
        egui::StrokeKind::Inside,
    );
    let time = if reduced() { 0.0 } else { time as f32 };
    let (from, to) = if indeterminate {
        let seg = (rect.width() * 0.26).max(52.0);
        let x = rect.left() - seg + (rect.width() + seg) * (time * 0.42).rem_euclid(1.0);
        (x.max(rect.left()), (x + seg).min(rect.right()))
    } else {
        (
            rect.left(),
            rect.left() + rect.width() * fraction.clamp(0.0, 1.0),
        )
    };
    if to - from > 1.0 {
        let fill_rect = Rect::from_min_max(pos2(from, rect.top()), pos2(to, rect.bottom()));
        crate::backdrop::paint_progress_fill(ui.ctx(), painter, fill_rect, 4);
    }
}
