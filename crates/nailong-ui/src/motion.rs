//! 基础控件和动效工具。
//!
//! 过渡都很短，结束后不再请求重绘，所以空闲时 CPU 是 0。

use eframe::egui;
use egui::{Color32, Rect, Sense, Shape, Stroke, pos2, vec2};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::theme as t;

/// 统一圆角刻度：基础控件 7、卡片/缩略图 10、弹窗 12。
pub const R: u8 = 7;
pub const R_CARD: u8 = 10;
pub const R_MODAL: u8 = 12;

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 柔顺对称缓动：适合退出层透明度平滑收敛。
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
    }
}

#[derive(Clone, Copy)]
struct SpringRectState {
    x: [f32; 4],
    v: [f32; 4],
    last_time: f64,
}

#[derive(Clone, Copy)]
struct Spring1DState {
    x: f32,
    v: f32,
    last_time: f64,
}

/// 临界阻尼弹簧矩形：让侧栏选中项、分段胶囊、下划线与命令面板光标在各项之间丝滑滑行；
/// 使用真实单调时钟 `now - last_time` 积分，在 60/120/144/240Hz 及无垂直同步环境下速度完全一致；
/// 静息唤醒首帧 `dt = 0`（零起跳），到位后立即停止重绘（空闲 CPU 仍为 0%）。
pub fn spring_rect(ctx: &egui::Context, id: egui::Id, target: Rect) -> Rect {
    if reduced() {
        return target;
    }
    let now = ctx.input(|input| input.time);
    let goal = [target.min.x, target.min.y, target.max.x, target.max.y];
    let Some(mut st) = ctx.data(|d| d.get_temp::<SpringRectState>(id)) else {
        ctx.data_mut(|d| {
            d.insert_temp(
                id,
                SpringRectState {
                    x: goal,
                    v: [0.0; 4],
                    last_time: now,
                },
            );
        });
        return target;
    };
    let jump = (goal[0] - st.x[0]).abs().max((goal[1] - st.x[1]).abs());
    if jump > 520.0 {
        ctx.data_mut(|d| {
            d.insert_temp(
                id,
                SpringRectState {
                    x: goal,
                    v: [0.0; 4],
                    last_time: now,
                },
            );
        });
        return target;
    }
    let elapsed = (now - st.last_time).max(0.0);
    // 空闲超过 80ms 刚被唤醒的首帧不跨大步，从当前起点平滑起步。
    let dt = if elapsed > 0.08 {
        0.0
    } else {
        (elapsed as f32).min(1.0 / 24.0)
    };
    let stiffness = 280.0_f32;
    let damping = 32.0_f32;
    let steps = 4;
    let h = dt / steps as f32;
    let mut moving = false;
    for ((x, v), &g) in st.x.iter_mut().zip(&mut st.v).zip(&goal) {
        if dt > 0.0 {
            for _ in 0..steps {
                *v += (stiffness * (g - *x) - damping * *v) * h;
                *x += *v * h;
            }
        }
        if (*x - g).abs() < 0.2 && v.abs() < 1.2 {
            *x = g;
            *v = 0.0;
        } else {
            moving = true;
        }
    }
    if moving {
        ctx.request_repaint();
    }
    st.last_time = now;
    ctx.data_mut(|d| d.insert_temp(id, st));
    Rect::from_min_max(pos2(st.x[0], st.x[1]), pos2(st.x[2], st.x[3]))
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

/// 物理弹簧（用于状态圆环与滑动开关）；采用真实单调时钟积分，运动中才请求下一帧。
pub fn spring(ctx: &egui::Context, id: egui::Id, target: f32, stiffness: f32, damping: f32) -> f32 {
    if reduced() {
        return target;
    }
    let now = ctx.input(|input| input.time);
    let mut st = ctx
        .data(|data| data.get_temp::<Spring1DState>(id))
        .unwrap_or(Spring1DState {
            x: target,
            v: 0.0,
            last_time: now,
        });
    let elapsed = (now - st.last_time).max(0.0);
    let dt = if elapsed > 0.08 {
        0.0
    } else {
        (elapsed as f32).min(1.0 / 24.0)
    };
    let steps = 4;
    let h = dt / steps as f32;
    if dt > 0.0 {
        for _ in 0..steps {
            st.v += (stiffness * (target - st.x) - damping * st.v) * h;
            st.x += st.v * h;
        }
    }
    if (st.x - target).abs() < 0.002 && st.v.abs() < 0.01 {
        st.x = target;
        st.v = 0.0;
    } else {
        ctx.request_repaint();
    }
    st.last_time = now;
    ctx.data_mut(|data| data.insert_temp(id, st));
    st.x
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

/// 主按钮：一屏最多一个，只变颜色，不位移。
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
    let (fill, color) = if !enabled {
        (t::wash(), t::dim())
    } else if pressed {
        (t::accent_press(), t::on_accent())
    } else {
        (mix(t::accent(), t::accent_hover(), hover), t::on_accent())
    };
    ui.painter().rect_filled(rect, R, fill);
    if enabled {
        // 顶部 1px 轻高光线 + 克制描边，给实色主按钮一点物理厚度。
        let top_highlight = Color32::from_white_alpha(if t::is_light() { 58 } else { 44 });
        let rim_x0 = rect.left() + R as f32;
        let rim_x1 = rect.right() - R as f32;
        if rim_x1 > rim_x0 && !pressed {
            ui.painter().line_segment(
                [
                    pos2(rim_x0, rect.top() + 1.0),
                    pos2(rim_x1, rect.top() + 1.0),
                ],
                Stroke::new(1.0, top_highlight),
            );
        }
        let edge = if pressed {
            t::accent_press()
        } else {
            mix(t::tint(t::accent_hover(), 0.7), t::accent_hover(), hover)
        };
        ui.painter()
            .rect_stroke(rect, R, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    }
    paint_label(ui.painter(), rect, text, glyph, color, 14.0, true);
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

/// 次按钮：克制表面底 + 细边，悬停和按下有平滑层级过渡。
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
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        mix(t::wash(), t::control(), 0.28)
    } else {
        mix(t::tint(t::surface(), 0.72), t::wash(), hover)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, R, fill);
    }
    ui.painter().rect_stroke(
        rect,
        R,
        Stroke::new(1.0, mix(t::hairline(), t::control(), hover)),
        egui::StrokeKind::Inside,
    );
    let color = if enabled {
        mix(t::ink(), t::ink(), hover)
    } else {
        t::dim()
    };
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

/// 高 5 的细进度条。
pub fn progress_bar(ui: &mut egui::Ui, fraction: f32, indeterminate: bool, time: f64) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 5.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3, t::hairline());
    let time = if reduced() { 0.0 } else { time as f32 };
    let (from, to) = if indeterminate {
        let seg = (rect.width() * 0.25).max(48.0);
        let x = rect.left() - seg + (rect.width() + seg) * (time * 0.4).rem_euclid(1.0);
        (x.max(rect.left()), (x + seg).min(rect.right()))
    } else {
        (
            rect.left(),
            rect.left() + rect.width() * fraction.clamp(0.0, 1.0),
        )
    };
    if to - from > 0.5 {
        painter.rect_filled(
            Rect::from_min_max(pos2(from, rect.top()), pos2(to, rect.bottom())),
            3,
            t::accent(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_rect_glides_smoothly_without_snapping() {
        set_reduced(false);
        let ctx = egui::Context::default();
        let id = egui::Id::unique("test_spring_rect");
        let a = Rect::from_min_size(pos2(12.0, 100.0), vec2(172.0, 36.0));
        let b = Rect::from_min_size(pos2(12.0, 220.0), vec2(172.0, 36.0));

        let step_run = |time: f64, target: Rect| -> Rect {
            let mut out = Rect::NOTHING;
            let raw = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut full = ctx.run_ui(raw, |ui| {
                out = spring_rect(ui.ctx(), id, target);
            });
            full.textures_delta.clear();
            out
        };

        // 首次记录起点 a。
        let out = step_run(1.0, a);
        assert!((out.min.y - 100.0).abs() < 0.01);

        // 空闲 0.5s 后目标切到 b：首帧平滑起步不瞬移。
        let out = step_run(1.5, b);
        assert!((out.min.y - 100.0).abs() < 0.01);

        // 50ms 后应处在中途滑行状态，而不是直接跳到终点 220。
        let mut out = Rect::NOTHING;
        for step in 1..=3 {
            out = step_run(1.5 + step as f64 * 0.016, b);
        }
        assert!(
            out.min.y > 105.0 && out.min.y < 190.0,
            "got y={}",
            out.min.y
        );
    }
}
