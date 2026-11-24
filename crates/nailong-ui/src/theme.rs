//! 雾蓝主题：深浅两套颜色共用同一组 token，色值见 `docs/design/18-ui-fog.md`。
//!
//! 除了这个文件，界面代码不写死颜色。

use eframe::egui;
use egui::Color32;
use std::sync::atomic::{AtomicBool, Ordering};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn white_alpha(a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(a, a, a, a)
}

#[derive(Clone, Copy)]
struct Palette {
    rail: Color32,
    bg: Color32,
    surface: Color32,
    elevated: Color32,
    wash: Color32,
    hairline: Color32,
    rim: Color32,
    control: Color32,
    ink: Color32,
    sub: Color32,
    dim: Color32,
    dot: Color32,
    accent: Color32,
    accent_hover: Color32,
    accent_press: Color32,
    on_accent: Color32,
    ok: Color32,
    warn: Color32,
    danger: Color32,
}

const DARK: Palette = Palette {
    rail: rgb(0x15, 0x18, 0x1E),
    bg: rgb(0x1B, 0x1E, 0x25),
    surface: rgb(0x22, 0x26, 0x2F),
    elevated: rgb(0x27, 0x2C, 0x36),
    wash: rgb(0x2B, 0x30, 0x3C),
    hairline: rgb(0x30, 0x36, 0x43),
    rim: white_alpha(18),
    control: rgb(0x6B, 0x72, 0x82),
    ink: rgb(0xE6, 0xE9, 0xF0),
    sub: rgb(0xA4, 0xAA, 0xB8),
    dim: rgb(0x8D, 0x94, 0xA4),
    dot: rgb(0x7D, 0x84, 0x94),
    accent: rgb(0x85, 0x9F, 0xD6),
    accent_hover: rgb(0x96, 0xAF, 0xE3),
    accent_press: rgb(0x73, 0x8D, 0xC4),
    on_accent: rgb(0x12, 0x15, 0x1B),
    ok: rgb(0x82, 0xB3, 0x9B),
    warn: rgb(0xCC, 0xA8, 0x72),
    danger: rgb(0xD4, 0x8A, 0x82),
};

const LIGHT: Palette = Palette {
    rail: rgb(0xE9, 0xEB, 0xE7),
    bg: rgb(0xF3, 0xF4, 0xF1),
    surface: rgb(0xFC, 0xFC, 0xFA),
    elevated: rgb(0xF5, 0xF6, 0xF3),
    wash: rgb(0xE4, 0xE7, 0xE4),
    hairline: rgb(0xD8, 0xDC, 0xE0),
    rim: white_alpha(180),
    control: rgb(0x7D, 0x83, 0x90),
    ink: rgb(0x1F, 0x23, 0x2A),
    sub: rgb(0x54, 0x5A, 0x66),
    dim: rgb(0x64, 0x6A, 0x75),
    dot: rgb(0x82, 0x88, 0x94),
    accent: rgb(0x4A, 0x63, 0x99),
    accent_hover: rgb(0x56, 0x70, 0xA8),
    accent_press: rgb(0x3E, 0x55, 0x87),
    on_accent: rgb(0xF8, 0xF9, 0xFB),
    ok: rgb(0x42, 0x73, 0x5B),
    warn: rgb(0x87, 0x66, 0x2B),
    danger: rgb(0xA8, 0x4E, 0x46),
};

static IS_LIGHT: AtomicBool = AtomicBool::new(false);

fn palette() -> Palette {
    if IS_LIGHT.load(Ordering::Relaxed) {
        LIGHT
    } else {
        DARK
    }
}

macro_rules! tokens {
    ($($name:ident),* $(,)?) => {
        $(pub fn $name() -> Color32 {
            palette().$name
        })*
    };
}

tokens!(
    rail,
    bg,
    surface,
    elevated,
    wash,
    hairline,
    rim,
    control,
    ink,
    sub,
    dim,
    dot,
    accent,
    accent_hover,
    accent_press,
    on_accent,
    ok,
    warn,
    danger,
);

pub fn is_light() -> bool {
    IS_LIGHT.load(Ordering::Relaxed)
}

/// 弹窗和浮层卡片的柔和投影。
pub fn modal_shadow() -> egui::epaint::Shadow {
    if is_light() {
        egui::epaint::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(32),
        }
    } else {
        egui::epaint::Shadow {
            offset: [0, 14],
            blur: 36,
            spread: 0,
            color: Color32::from_black_alpha(120),
        }
    }
}

/// 把颜色调成半透明，用于状态提示的底色和边框。
pub fn tint(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

/// 弹窗后面的遮罩。
pub fn veil(alpha: f32) -> Color32 {
    let alpha = alpha.clamp(0.0, 1.0);
    if IS_LIGHT.load(Ordering::Relaxed) {
        Color32::from_black_alpha((alpha * 90.0) as u8)
    } else {
        Color32::from_black_alpha((alpha * 150.0) as u8)
    }
}

/// 外观设置，存在 `ui.json` 里。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preference {
    System,
    Dark,
    Light,
}

impl Preference {
    pub const ALL: [Preference; 3] = [Preference::System, Preference::Dark, Preference::Light];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Dark => "深色",
            Self::Light => "浅色",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(match self {
            Self::System => egui::ThemePreference::System,
            Self::Dark => egui::ThemePreference::Dark,
            Self::Light => egui::ThemePreference::Light,
        });
        sync(ctx);
    }
}

/// 每帧开始时调用：系统外观可能在运行中变化。
pub fn sync(ctx: &egui::Context) {
    IS_LIGHT.store(ctx.theme() == egui::Theme::Light, Ordering::Relaxed);
}

fn visuals(p: Palette, dark: bool) -> egui::Visuals {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let radius = egui::CornerRadius::same(7);
    let shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 22,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 95 } else { 30 }),
    };
    visuals.override_text_color = Some(p.ink);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.extreme_bg_color = p.surface;
    visuals.window_stroke = egui::Stroke::new(1.0, p.hairline);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(9);
    visuals.popup_shadow = shadow;
    visuals.window_shadow = shadow;
    visuals.faint_bg_color = p.wash;
    visuals.code_bg_color = p.wash;
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.slider_trailing_fill = true;
    visuals.text_cursor.stroke = egui::Stroke::new(2.0, p.accent);
    let widget = |fill: Color32, stroke: Color32| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: egui::Stroke::new(1.0, stroke),
        corner_radius: radius,
        fg_stroke: egui::Stroke::new(1.0, p.ink),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = widget(Color32::TRANSPARENT, p.hairline);
    visuals.widgets.inactive = widget(p.surface, p.hairline);
    visuals.widgets.hovered = widget(p.wash, p.control);
    visuals.widgets.active = widget(p.wash, p.accent);
    visuals.widgets.open = widget(p.wash, p.accent);
    visuals
}

/// 启动时注册两套样式；之后只需切换 `ThemePreference`。
pub fn install(ctx: &egui::Context) {
    for (theme, palette, dark) in [
        (egui::Theme::Dark, DARK, true),
        (egui::Theme::Light, LIGHT, false),
    ] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = visuals(palette, dark);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        style.animation_time = 0.12;
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(24.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(13.0, egui::FontFamily::Monospace),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
            ),
        ]
        .into();
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        // 文字默认不能选中：整行、整块可点的地方，点在字上也算点到它（以前会被选字吃掉）。
        // 要复制的地方（日志、错误提示）单独打开。
        style.interaction.selectable_labels = false;
        // egui 自带的按钮也和我们自己画的一样，悬停时变成手形。
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        ctx.set_style_of(theme, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn both_palettes_meet_the_spec_contrast() {
        for p in [DARK, LIGHT] {
            for text in [p.ink, p.sub, p.dim, p.accent, p.ok, p.warn, p.danger] {
                assert!(contrast(text, p.bg) >= 4.5, "{text:?} on bg");
            }
            for text in [p.ink, p.sub, p.dim] {
                assert!(contrast(text, p.surface) >= 4.5, "{text:?} on surface");
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
            assert!(contrast(p.control, p.bg) >= 3.0);
        }
    }

    #[test]
    fn preference_round_trips_through_its_key() {
        for pref in Preference::ALL {
            assert_eq!(Preference::from_key(pref.key()), pref);
        }
        assert_eq!(Preference::from_key("anything"), Preference::System);
    }
}
