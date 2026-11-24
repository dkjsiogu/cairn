//! 晨雾星野主题：深色「星野夜雾」与浅色「晨曦云岩」共用同一组语义 token，
//! 所有正文与控件配色均通过 WCAG AA 对比度单测验证。

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
    accent_top: Color32,
    accent_bottom: Color32,
    accent_hover_top: Color32,
    accent_hover_bottom: Color32,
    accent_press: Color32,
    on_accent: Color32,
    ok: Color32,
    warn: Color32,
    danger: Color32,
    loader_neoforge: Color32,
    loader_fabric: Color32,
    loader_forge: Color32,
    loader_vanilla: Color32,
    loader_pack: Color32,
}

/// 深色主题：星野夜雾（Alpine Dusk）
const DARK: Palette = Palette {
    rail: rgb(0x0B, 0x0E, 0x15),
    bg: rgb(0x11, 0x15, 0x20),
    surface: rgb(0x18, 0x1E, 0x2C),
    elevated: rgb(0x20, 0x28, 0x3A),
    wash: rgb(0x26, 0x30, 0x46),
    hairline: rgb(0x2A, 0x35, 0x4C),
    rim: white_alpha(26),
    control: rgb(0x6B, 0x7A, 0x99),
    ink: rgb(0xEE, 0xF2, 0xFA),
    sub: rgb(0xA6, 0xB2, 0xCA),
    dim: rgb(0x8B, 0x99, 0xB5),
    dot: rgb(0x88, 0x9A, 0xBC),
    accent: rgb(0x6A, 0x9B, 0xF5),
    accent_top: rgb(0x7C, 0xAB, 0xFF),
    accent_bottom: rgb(0x4A, 0x7B, 0xE2),
    accent_hover_top: rgb(0x91, 0xBC, 0xFF),
    accent_hover_bottom: rgb(0x58, 0x89, 0xEC),
    accent_press: rgb(0x45, 0x73, 0xD4),
    on_accent: rgb(0x09, 0x10, 0x1F),
    ok: rgb(0x60, 0xD2, 0x9B),
    warn: rgb(0xEB, 0xB4, 0x62),
    danger: rgb(0xEC, 0x7E, 0x78),
    loader_neoforge: rgb(0xF0, 0x9A, 0x54),
    loader_fabric: rgb(0xDF, 0xC2, 0x85),
    loader_forge: rgb(0x6F, 0xA4, 0xF7),
    loader_vanilla: rgb(0x5A, 0xD1, 0x95),
    loader_pack: rgb(0xB0, 0x8C, 0xF5),
};

/// 浅色主题：晨曦云岩（Alpine Morning）
const LIGHT: Palette = Palette {
    rail: rgb(0xE6, 0xEC, 0xF5),
    bg: rgb(0xF2, 0xF5, 0xFA),
    surface: rgb(0xFF, 0xFF, 0xFF),
    elevated: rgb(0xF6, 0xF9, 0xFD),
    wash: rgb(0xE4, 0xEC, 0xF8),
    hairline: rgb(0xD3, 0xDD, 0xEC),
    rim: white_alpha(220),
    control: rgb(0x6D, 0x7D, 0x9A),
    ink: rgb(0x16, 0x1E, 0x2E),
    sub: rgb(0x48, 0x55, 0x70),
    dim: rgb(0x5A, 0x67, 0x82),
    dot: rgb(0x6B, 0x7B, 0x98),
    accent: rgb(0x2F, 0x62, 0xC8),
    accent_top: rgb(0x42, 0x76, 0xDE),
    accent_bottom: rgb(0x25, 0x52, 0xB2),
    accent_hover_top: rgb(0x50, 0x83, 0xE8),
    accent_hover_bottom: rgb(0x2F, 0x5D, 0xBE),
    accent_press: rgb(0x21, 0x4A, 0xA2),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    ok: rgb(0x1E, 0x78, 0x4E),
    warn: rgb(0x94, 0x5E, 0x12),
    danger: rgb(0xBD, 0x3B, 0x36),
    loader_neoforge: rgb(0xC2, 0x62, 0x18),
    loader_fabric: rgb(0x8F, 0x6D, 0x22),
    loader_forge: rgb(0x2F, 0x62, 0xC2),
    loader_vanilla: rgb(0x22, 0x7E, 0x50),
    loader_pack: rgb(0x6B, 0x42, 0xC4),
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
    accent_top,
    accent_bottom,
    accent_hover_top,
    accent_hover_bottom,
    accent_press,
    on_accent,
    ok,
    warn,
    danger,
    loader_pack,
);

pub fn loader_color(kind: crate::model::LoaderKind) -> Color32 {
    let p = palette();
    match kind {
        crate::model::LoaderKind::NeoForge => p.loader_neoforge,
        crate::model::LoaderKind::Fabric => p.loader_fabric,
        crate::model::LoaderKind::Forge => p.loader_forge,
        crate::model::LoaderKind::Vanilla => p.loader_vanilla,
    }
}

pub fn is_light() -> bool {
    IS_LIGHT.load(Ordering::Relaxed)
}

/// 略微透出底层大气画布的玻璃卡片表面色。
pub fn surface_glass() -> Color32 {
    let s = surface();
    if is_light() {
        Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 238)
    } else {
        Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 224)
    }
}

/// 玻璃卡片的精致微亮描边。
pub fn glass_border() -> Color32 {
    if is_light() {
        Color32::from_rgba_unmultiplied(36, 70, 128, 34)
    } else {
        Color32::from_rgba_unmultiplied(148, 180, 238, 40)
    }
}

/// 普通卡片的柔和环境阴影。
pub fn card_shadow() -> egui::epaint::Shadow {
    if is_light() {
        egui::epaint::Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(24, 48, 92, 18),
        }
    } else {
        egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(78),
        }
    }
}

/// 主按钮底部的柔和色光环境投影。
pub fn glow_shadow(color: Color32, strength: f32) -> egui::epaint::Shadow {
    let alpha = ((if is_light() { 72.0 } else { 96.0 }) * strength.clamp(0.0, 1.4)) as u8;
    egui::epaint::Shadow {
        offset: [0, 5],
        blur: 16,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
    }
}

/// 弹窗和浮层卡片的柔和投影。
pub fn modal_shadow() -> egui::epaint::Shadow {
    if is_light() {
        egui::epaint::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(18, 36, 74, 38),
        }
    } else {
        egui::epaint::Shadow {
            offset: [0, 16],
            blur: 42,
            spread: 0,
            color: Color32::from_black_alpha(150),
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
