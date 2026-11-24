//! GPU 缓存的程序化大气画布、地平线英雄横幅与圆角渐变纹理。
//!
//! 所有纹理按主题（深色「星野夜雾」/ 浅色「晨曦云岩」）只生成一次并常驻显存，
//! 不逐帧计算、不触发空闲重绘，保持 0% 空闲 CPU。

use eframe::egui;
use egui::{Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, Vec2, pos2, vec2};

use crate::theme as t;

const UV_FULL: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

#[derive(Clone)]
struct CachedTextures {
    light: bool,
    canvas: TextureHandle,
    hero: TextureHandle,
    primary_idle: TextureHandle,
    primary_hover: TextureHandle,
    primary_press: TextureHandle,
    progress_fill: TextureHandle,
    nav_active: TextureHandle,
    hero_scrim: TextureHandle,
}

fn cache(ctx: &egui::Context) -> CachedTextures {
    let light = t::is_light();
    let id = egui::Id::unique("cairn_backdrop_cache_v1");
    if let Some(existing) = ctx.data(|d| d.get_temp::<CachedTextures>(id))
        && existing.light == light
    {
        return existing;
    }
    let suffix = if light { "light" } else { "dark" };
    let load = |name: &str, img: ColorImage| {
        ctx.load_texture(
            format!("cairn_{name}_{suffix}"),
            img,
            TextureOptions::LINEAR,
        )
    };
    let built = CachedTextures {
        light,
        canvas: load("canvas", build_canvas_image(light)),
        hero: load("hero", build_hero_image(light)),
        primary_idle: load(
            "btn_idle",
            build_v_gradient(t::accent_top(), t::accent_bottom()),
        ),
        primary_hover: load(
            "btn_hover",
            build_v_gradient(t::accent_hover_top(), t::accent_hover_bottom()),
        ),
        primary_press: load(
            "btn_press",
            build_v_gradient(t::accent_press(), t::accent_bottom()),
        ),
        progress_fill: load(
            "progress",
            build_h_gradient(
                t::accent_bottom(),
                t::accent_top(),
                if light {
                    Color32::from_rgb(0x46, 0xA8, 0xD8)
                } else {
                    Color32::from_rgb(0x74, 0xD8, 0xC8)
                },
            ),
        ),
        nav_active: load("nav_active", build_nav_active_image(light)),
        hero_scrim: load("hero_scrim", build_hero_scrim_image(light)),
    };
    ctx.data_mut(|d| d.insert_temp(id, built.clone()));
    built
}

/// 在主内容区底部铺设柔和的程序化大气画布（零色带微噪点，GPU 线性插值）。
pub(crate) fn paint_canvas(ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
    let tex = cache(ctx);
    painter.image(tex.canvas.id(), rect, UV_FULL, Color32::WHITE);
}

/// 在启动台英雄舞台卡片里绘制圆角裁剪的「晨雾星野 / 晨曦云岩」地平线全景图与左侧可读性遮罩。
pub(crate) fn paint_hero_stage(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
) {
    let tex = cache(ctx);
    // 1. 柔和外部环境投影
    painter.add(t::card_shadow().as_shape(rect, radius));
    // 2. 全景大气山脊底图（自带完美抗锯齿圆角）
    painter.add(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(tex.hero.id(), UV_FULL),
    );
    // 3. 左侧平滑可读性过渡层，确保大标题和启动参数对比度充裕
    painter.add(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(tex.hero_scrim.id(), UV_FULL),
    );
    // 4. 顶部 1px 玻璃高光边 + 精致描边
    let r = radius as f32;
    if rect.width() > r * 2.0 {
        painter.line_segment(
            [
                pos2(rect.left() + r, rect.top() + 1.0),
                pos2(rect.right() - r, rect.top() + 1.0),
            ],
            egui::Stroke::new(1.0, t::rim()),
        );
    }
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, t::glass_border()),
        egui::StrokeKind::Inside,
    );
}

/// 绘制带圆角的纵向渐变主按钮底色（支持常驻 / 悬停 / 按下三态）。
pub(crate) fn paint_primary_fill(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    hover: f32,
    pressed: bool,
) {
    let tex = cache(ctx);
    let handle = if pressed {
        &tex.primary_press
    } else if hover > 0.5 {
        &tex.primary_hover
    } else {
        &tex.primary_idle
    };
    painter.add(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(handle.id(), UV_FULL),
    );
    if !pressed && hover > 0.01 && hover <= 0.5 {
        let tint = Color32::from_white_alpha((hover * 2.0 * 32.0) as u8);
        painter.rect_filled(rect, radius, tint);
    }
}

/// 绘制带圆角的横向渐变进度条填充。
pub(crate) fn paint_progress_fill(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
) {
    let tex = cache(ctx);
    painter.add(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(tex.progress_fill.id(), UV_FULL),
    );
}

/// 侧边栏当前激活项的横向柔光底（返回 Shape，供底层弹簧滑行槽位复用）。
pub(crate) fn nav_active_shape(ctx: &egui::Context, rect: Rect, radius: u8) -> egui::Shape {
    let tex = cache(ctx);
    let fill = egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
        .with_texture(tex.nav_active.id(), UV_FULL);
    let stroke = egui::epaint::RectShape::stroke(
        rect,
        radius,
        egui::Stroke::new(
            1.0,
            t::tint(t::accent(), if t::is_light() { 0.28 } else { 0.30 }),
        ),
        egui::StrokeKind::Inside,
    );
    egui::Shape::Vec(vec![fill.into(), stroke.into()])
}

/// 侧边栏当前激活项的横向柔光底。
pub(crate) fn paint_nav_active(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
) {
    painter.add(nav_active_shape(ctx, rect, radius));
}

/// 绘制带阴影、微透明层次与顶部 1px 玻璃高光的卡片面板。
pub(crate) fn paint_glass_card(
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    fill: Color32,
    border: Color32,
    with_shadow: bool,
) {
    if with_shadow {
        painter.add(t::card_shadow().as_shape(rect, radius));
    }
    painter.rect(
        rect,
        radius,
        fill,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let r = radius as f32;
    if rect.width() > r * 2.0 {
        painter.line_segment(
            [
                pos2(rect.left() + r, rect.top() + 1.0),
                pos2(rect.right() - r, rect.top() + 1.0),
            ],
            egui::Stroke::new(1.0, t::rim()),
        );
    }
}

/// 确定性低频哈希噪点（-1..=1），消除 8-bit 渐变色带。
fn hash_noise(x: usize, y: usize) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    let u = ((n ^ (n >> 16)) & 0xFFFF) as f32 / 65535.0;
    u * 2.0 - 1.0
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add_glow(base: [f32; 3], glow: [f32; 3], strength: f32) -> [f32; 3] {
    let s = strength.clamp(0.0, 1.0);
    [
        (base[0] + glow[0] * s).clamp(0.0, 255.0),
        (base[1] + glow[1] * s).clamp(0.0, 255.0),
        (base[2] + glow[2] * s).clamp(0.0, 255.0),
    ]
}

fn radial_falloff(u: f32, v: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let dx = (u - cx) / rx;
    let dy = (v - cy) / ry;
    let d2 = dx * dx + dy * dy;
    (-d2 * 1.65).exp()
}

fn to_color(rgb: [f32; 3], dither: f32) -> Color32 {
    Color32::from_rgb(
        (rgb[0] + dither).clamp(0.0, 255.0) as u8,
        (rgb[1] + dither).clamp(0.0, 255.0) as u8,
        (rgb[2] + dither).clamp(0.0, 255.0) as u8,
    )
}

/// 主内容区底层大气画布：深色为星野深蓝微极光，浅色为晨曦云岩柔光。
fn build_canvas_image(light: bool) -> ColorImage {
    let w = 240usize;
    let h = 150usize;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let v = y as f32 / (h - 1) as f32;
        for x in 0..w {
            let u = x as f32 / (w - 1) as f32;
            let dither = hash_noise(x, y) * 0.9;
            let c = if light {
                let mut rgb = lerp3([244.0, 247.0, 252.0], [233.0, 239.0, 248.0], v);
                // 右上淡冰川天光
                let g1 = radial_falloff(u, v, 0.80, 0.16, 0.50, 0.45);
                rgb = lerp3(rgb, [212.0, 229.0, 252.0], g1 * 0.62);
                // 左下暖云岩漫反射
                let g2 = radial_falloff(u, v, 0.14, 0.86, 0.52, 0.48);
                rgb = lerp3(rgb, [249.0, 241.0, 231.0], g2 * 0.55);
                rgb
            } else {
                let mut rgb = lerp3([15.0, 20.0, 31.0], [18.0, 23.0, 34.0], v);
                // 右上星野蓝雾光晕
                let g1 = radial_falloff(u, v, 0.82, 0.18, 0.50, 0.48);
                rgb = add_glow(rgb, [34.0, 62.0, 118.0], g1 * 0.72);
                // 中左极淡青岚
                let g2 = radial_falloff(u, v, 0.14, 0.82, 0.46, 0.44);
                rgb = add_glow(rgb, [14.0, 48.0, 58.0], g2 * 0.45);
                // 顶中极微暮紫过渡
                let g3 = radial_falloff(u, v, 0.45, 0.04, 0.42, 0.30);
                rgb = add_glow(rgb, [28.0, 32.0, 74.0], g3 * 0.36);
                rgb
            };
            pixels.push(to_color(c, dither));
        }
    }
    ColorImage {
        size: [w, h],
        source_size: vec2(w as f32, h as f32),
        pixels,
    }
}

/// 山脊轮廓高度函数（归一化 y，越大越靠下）。
fn ridge_y(u: f32, base: f32, amp: f32, f1: f32, p1: f32, f2: f32, p2: f32) -> f32 {
    let s1 = (u * f1 + p1).sin();
    let s2 = (u * f2 + p2).sin() * 0.45;
    let s3 = (u * f2 * 2.13 + p1 * 1.7).cos() * 0.18;
    // 尖锐一点的阿尔卑斯山峰感
    let peak = (1.0 - s1.abs()).powf(1.35) * 0.65 + (s2 + s3) * 0.35;
    base - amp * peak
}

/// 启动台英雄舞台全景图：柔和极光/晨曦天空 + 远中近三层山脊剪影 + 星空/云带。
fn build_hero_image(light: bool) -> ColorImage {
    let w = 520usize;
    let h = 180usize;
    let mut pixels = vec![Color32::TRANSPARENT; w * h];

    for y in 0..h {
        let v = y as f32 / (h - 1) as f32;
        for x in 0..w {
            let u = x as f32 / (w - 1) as f32;
            let dither = hash_noise(x, y) * 1.1;

            let mut rgb = if light {
                // 晨曦晴空到柔和地平线
                let mut sky = lerp3([214.0, 229.0, 250.0], [244.0, 248.0, 254.0], v.powf(0.85));
                // 右侧圆环背光：淡天青晨光
                let halo = radial_falloff(u, v, 0.80, 0.48, 0.30, 0.56);
                sky = lerp3(sky, [186.0, 214.0, 252.0], halo * 0.68);
                // 左侧微暖晨曦
                let warm = radial_falloff(u, v, 0.20, 0.25, 0.38, 0.45);
                sky = lerp3(sky, [252.0, 244.0, 232.0], warm * 0.42);
                // 轻柔卷云丝带
                let cloud_center = 0.28 + 0.06 * (u * 5.2 + 0.6).sin();
                let cloud = (-((v - cloud_center) / 0.09).powi(2)).exp()
                    * (0.5 + 0.5 * (u * 9.0 + 1.2).cos());
                sky = lerp3(sky, [255.0, 255.0, 255.0], cloud * 0.34);
                sky
            } else {
                // 深夜蓝黑天空 -> 地平线冷岚
                let mut sky = lerp3([18.0, 25.0, 42.0], [25.0, 36.0, 58.0], v);
                // 北方丝带极光（温和的高级蓝青，非霓虹）
                let ribbon_y = 0.28 + 0.11 * (u * 4.4 - 0.5).sin() + 0.04 * (u * 9.5).cos();
                let ribbon = (-((v - ribbon_y) / 0.13).powi(2)).exp()
                    * (0.45 + 0.55 * (u * 3.2 - 0.2).sin().max(0.0));
                sky = add_glow(sky, [56.0, 118.0, 178.0], ribbon * 0.52);
                sky = add_glow(sky, [44.0, 154.0, 138.0], ribbon * (u * 0.8) * 0.26);
                // 右侧状态圆环的舞台聚光晕
                let ring_glow = radial_falloff(u, v, 0.81, 0.50, 0.26, 0.56);
                sky = add_glow(sky, [66.0, 112.0, 206.0], ring_glow * 0.54);
                // 左下暖色深沉反衬
                let left_depth = radial_falloff(u, v, 0.15, 0.85, 0.35, 0.45);
                sky = add_glow(sky, [20.0, 30.0, 54.0], left_depth * 0.30);
                sky
            };

            // 三层渐远阿尔卑斯山脊（只落在下半部，为文字和圆环留足呼吸空间）
            let far_y = ridge_y(u, 0.84, 0.21, 5.2, 0.4, 13.5, 1.9);
            let mid_y = ridge_y(u, 0.91, 0.17, 4.1, 2.3, 11.2, 0.5);
            let near_y = ridge_y(u, 0.97, 0.12, 3.6, 4.1, 9.4, 2.8);

            if v >= far_y {
                let mist = ((v - far_y) / 0.16).clamp(0.0, 1.0);
                let col = if light {
                    lerp3([190.0, 208.0, 232.0], [218.0, 230.0, 245.0], mist * 0.5)
                } else {
                    lerp3([26.0, 38.0, 62.0], [22.0, 31.0, 50.0], mist)
                };
                rgb = lerp3(rgb, col, 0.78);
            }
            if v >= mid_y {
                let mist = ((v - mid_y) / 0.14).clamp(0.0, 1.0);
                let col = if light {
                    lerp3([174.0, 195.0, 224.0], [206.0, 220.0, 240.0], mist * 0.45)
                } else {
                    lerp3([21.0, 30.0, 48.0], [18.0, 25.0, 40.0], mist)
                };
                rgb = lerp3(rgb, col, 0.86);
            }
            if v >= near_y {
                let col = if light {
                    [160.0, 184.0, 216.0]
                } else {
                    [16.0, 22.0, 35.0]
                };
                rgb = lerp3(rgb, col, 0.92);
            }

            pixels[y * w + x] = to_color(rgb, dither);
        }
    }

    // 深色模式下点缀稀疏克制的微星光（只落在山脊之上的夜空）
    if !light {
        const STARS: [(f32, f32, f32); 22] = [
            (0.08, 0.14, 0.55),
            (0.16, 0.24, 0.35),
            (0.23, 0.11, 0.65),
            (0.31, 0.19, 0.40),
            (0.38, 0.09, 0.50),
            (0.44, 0.28, 0.38),
            (0.51, 0.14, 0.72),
            (0.57, 0.24, 0.45),
            (0.62, 0.11, 0.55),
            (0.67, 0.20, 0.42),
            (0.71, 0.08, 0.78),
            (0.76, 0.16, 0.48),
            (0.83, 0.10, 0.62),
            (0.89, 0.18, 0.55),
            (0.94, 0.12, 0.70),
            (0.96, 0.28, 0.42),
            (0.54, 0.36, 0.35),
            (0.64, 0.34, 0.45),
            (0.47, 0.07, 0.42),
            (0.79, 0.06, 0.52),
            (0.91, 0.35, 0.38),
            (0.12, 0.38, 0.30),
        ];
        for (su, sv, bright) in STARS {
            let cx = (su * (w - 1) as f32) as isize;
            let cy = (sv * (h - 1) as f32) as isize;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px < 0 || py < 0 || px >= w as isize || py >= h as isize {
                        continue;
                    }
                    let falloff = if dx == 0 && dy == 0 {
                        1.0
                    } else if dx == 0 || dy == 0 {
                        0.32
                    } else {
                        0.08
                    };
                    let idx = py as usize * w + px as usize;
                    let cur = pixels[idx];
                    let add = bright * falloff * 185.0;
                    pixels[idx] = Color32::from_rgb(
                        (cur.r() as f32 + add * 0.85).min(255.0) as u8,
                        (cur.g() as f32 + add * 0.93).min(255.0) as u8,
                        (cur.b() as f32 + add).min(255.0) as u8,
                    );
                }
            }
        }
    }

    ColorImage {
        size: [w, h],
        source_size: vec2(w as f32, h as f32),
        pixels,
    }
}

/// 英雄舞台左侧平滑可读性渐变遮罩（左实右透）。
fn build_hero_scrim_image(light: bool) -> ColorImage {
    let w = 128usize;
    let mut pixels = Vec::with_capacity(w);
    let base = if light {
        Color32::from_rgb(248, 251, 255)
    } else {
        Color32::from_rgb(15, 20, 31)
    };
    for x in 0..w {
        let u = x as f32 / (w - 1) as f32;
        // 左侧 0..0.52 提供柔和护底，右侧平滑退成完全透明露出山脊和圆环光晕
        let a = if u < 0.15 {
            if light { 0.54 } else { 0.56 }
        } else if u < 0.68 {
            let t = (u - 0.15) / 0.53;
            let smooth = 1.0 - t * t * (3.0 - 2.0 * t);
            smooth * if light { 0.54 } else { 0.56 }
        } else {
            0.0
        };
        pixels.push(Color32::from_rgba_unmultiplied(
            base.r(),
            base.g(),
            base.b(),
            (a * 255.0) as u8,
        ));
    }
    ColorImage {
        size: [w, 1],
        source_size: vec2(w as f32, 1.0),
        pixels,
    }
}

fn build_v_gradient(top: Color32, bottom: Color32) -> ColorImage {
    let h = 48usize;
    let mut pixels = Vec::with_capacity(h);
    let a = [top.r() as f32, top.g() as f32, top.b() as f32];
    let b = [bottom.r() as f32, bottom.g() as f32, bottom.b() as f32];
    for y in 0..h {
        let t = y as f32 / (h - 1) as f32;
        let c = lerp3(a, b, t);
        pixels.push(Color32::from_rgb(c[0] as u8, c[1] as u8, c[2] as u8));
    }
    ColorImage {
        size: [1, h],
        source_size: vec2(1.0, h as f32),
        pixels,
    }
}

fn build_h_gradient(left: Color32, mid: Color32, right: Color32) -> ColorImage {
    let w = 64usize;
    let mut pixels = Vec::with_capacity(w);
    let a = [left.r() as f32, left.g() as f32, left.b() as f32];
    let b = [mid.r() as f32, mid.g() as f32, mid.b() as f32];
    let c = [right.r() as f32, right.g() as f32, right.b() as f32];
    for x in 0..w {
        let t = x as f32 / (w - 1) as f32;
        let rgb = if t < 0.65 {
            lerp3(a, b, t / 0.65)
        } else {
            lerp3(b, c, (t - 0.65) / 0.35)
        };
        pixels.push(Color32::from_rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8));
    }
    ColorImage {
        size: [w, 1],
        source_size: vec2(w as f32, 1.0),
        pixels,
    }
}

/// 侧边栏选中项的横向渐变微光底（左侧略亮强调，右侧渐融入侧栏）。
fn build_nav_active_image(light: bool) -> ColorImage {
    let w = 64usize;
    let mut pixels = Vec::with_capacity(w);
    let accent = t::accent();
    let wash = t::wash();
    for x in 0..w {
        let u = x as f32 / (w - 1) as f32;
        let mix_a = (1.0 - u) * if light { 0.16 } else { 0.24 };
        let r = wash.r() as f32 * (1.0 - mix_a) + accent.r() as f32 * mix_a;
        let g = wash.g() as f32 * (1.0 - mix_a) + accent.g() as f32 * mix_a;
        let b = wash.b() as f32 * (1.0 - mix_a) + accent.b() as f32 * mix_a;
        let alpha = if light { 235 } else { 220 };
        pixels.push(Color32::from_rgba_unmultiplied(
            r.clamp(0.0, 255.0) as u8,
            g.clamp(0.0, 255.0) as u8,
            b.clamp(0.0, 255.0) as u8,
            alpha,
        ));
    }
    ColorImage {
        size: [w, 1],
        source_size: vec2(w as f32, 1.0),
        pixels,
    }
}

/// 生成精巧的迷你方块世界占位缩略图（当世界没有 `icon.png` 时使用，带山丘与草方块的几何风景）。
pub(crate) fn paint_world_placeholder(painter: &egui::Painter, rect: Rect, seed: &str) {
    let mut h: u32 = 2166136261;
    for b in seed.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    let biome = h % 3;
    let (sky_top, sky_bot, hill_far, hill_near, sun) = match biome {
        // 晨雾山谷：清透碧空 + 翠绿草坡
        0 => (
            Color32::from_rgb(0x4C, 0x7A, 0xC4),
            Color32::from_rgb(0x88, 0xB4, 0xDE),
            Color32::from_rgb(0x46, 0x7D, 0x67),
            Color32::from_rgb(0x4E, 0x99, 0x5B),
            Color32::from_rgb(0xF8, 0xE5, 0xA8),
        ),
        // 暮色红石：晚霞橙紫 + 暖岩山丘
        1 => (
            Color32::from_rgb(0x3B, 0x35, 0x6B),
            Color32::from_rgb(0xC2, 0x6F, 0x5E),
            Color32::from_rgb(0x5A, 0x3A, 0x4E),
            Color32::from_rgb(0x3D, 0x2B, 0x3B),
            Color32::from_rgb(0xFB, 0xC9, 0x83),
        ),
        // 雪原冰峰：冰川蓝白 + 深松林
        _ => (
            Color32::from_rgb(0x2B, 0x4A, 0x78),
            Color32::from_rgb(0x7B, 0xA8, 0xD4),
            Color32::from_rgb(0x9E, 0xBB, 0xD6),
            Color32::from_rgb(0x2F, 0x5A, 0x66),
            Color32::from_rgb(0xE8, 0xF3, 0xFF),
        ),
    };
    let r = 8u8;
    painter.rect_filled(rect, r, sky_top);
    let lower_sky = Rect::from_min_max(pos2(rect.left(), rect.center().y - 4.0), rect.max);
    painter.rect_filled(lower_sky, r, sky_bot);
    // 小太阳 / 月亮
    painter.circle_filled(
        pos2(
            rect.right() - rect.width() * 0.28,
            rect.top() + rect.height() * 0.28,
        ),
        (rect.width() * 0.11).clamp(3.0, 6.0),
        sun,
    );
    // 远山
    let w = rect.width();
    let ht = rect.height();
    let p = |fx: f32, fy: f32| pos2(rect.left() + w * fx, rect.top() + ht * fy);
    painter.add(egui::Shape::convex_polygon(
        vec![
            p(0.0, 0.72),
            p(0.32, 0.44),
            p(0.68, 0.64),
            p(1.0, 0.48),
            p(1.0, 1.0),
            p(0.0, 1.0),
        ],
        hill_far,
        egui::Stroke::NONE,
    ));
    // 近坡（带圆角裁切用底层覆盖）
    let ground = Rect::from_min_max(pos2(rect.left(), rect.top() + ht * 0.68), rect.max);
    painter.rect_filled(
        ground,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: r,
            se: r,
        },
        hill_near,
    );
    painter.rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0, t::glass_border()),
        egui::StrokeKind::Inside,
    );
}

/// 绘制定制的加载器几何徽标（NeoForge 狐标 / Fabric 织纹 / Forge 铁砧 / 原版草块 / 整合包棱晶）。
pub(crate) fn paint_loader_icon(
    painter: &egui::Painter,
    center: Pos2,
    size: f32,
    loader: crate::model::LoaderKind,
    is_pack: bool,
    color: Color32,
) {
    let s = size / 18.0;
    let p = |x: f32, y: f32| center + Vec2::new(x * s, y * s);
    let stroke = egui::Stroke::new(1.5 * s.max(0.85), color);
    if is_pack {
        // 整合包：六边形水晶棱块
        painter.add(egui::Shape::closed_line(
            vec![
                p(0.0, -7.2),
                p(6.4, -3.5),
                p(6.4, 3.5),
                p(0.0, 7.2),
                p(-6.4, 3.5),
                p(-6.4, -3.5),
            ],
            stroke,
        ));
        painter.line_segment([p(-6.4, -3.5), p(0.0, 0.2)], stroke);
        painter.line_segment([p(6.4, -3.5), p(0.0, 0.2)], stroke);
        painter.line_segment([p(0.0, 0.2), p(0.0, 7.2)], stroke);
        return;
    }
    match loader {
        crate::model::LoaderKind::NeoForge => {
            // NeoForge：几何狐面轮廓
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-6.2, -5.8),
                    p(-1.6, -1.8),
                    p(1.6, -1.8),
                    p(6.2, -5.8),
                    p(5.4, 1.8),
                    p(0.0, 6.6),
                    p(-5.4, 1.8),
                ],
                stroke,
            ));
            painter.circle_filled(p(0.0, 2.2), 1.3 * s, color);
        }
        crate::model::LoaderKind::Fabric => {
            // Fabric：双层斜织卷轴
            painter.add(egui::Shape::closed_line(
                vec![p(-5.5, -4.5), p(5.5, -4.5), p(3.8, 5.2), p(-3.8, 5.2)],
                stroke,
            ));
            painter.line_segment([p(-4.5, -0.5), p(4.5, -0.5)], stroke);
            painter.line_segment([p(-3.8, 2.5), p(3.8, 2.5)], stroke);
        }
        crate::model::LoaderKind::Forge => {
            // Forge：铁砧轮廓
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-6.8, -4.5),
                    p(6.8, -4.5),
                    p(4.2, -0.5),
                    p(3.0, 2.2),
                    p(5.2, 5.2),
                    p(-5.2, 5.2),
                    p(-3.0, 2.2),
                    p(-4.2, -0.5),
                ],
                stroke,
            ));
        }
        crate::model::LoaderKind::Vanilla => {
            // 原版：等距方块
            painter.add(egui::Shape::closed_line(
                vec![
                    p(0.0, -6.8),
                    p(6.2, -3.2),
                    p(6.2, 3.4),
                    p(0.0, 6.8),
                    p(-6.2, 3.4),
                    p(-6.2, -3.2),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(-6.2, -3.2), p(0.0, 0.2), p(6.2, -3.2)],
                stroke,
            ));
            painter.line_segment([p(0.0, 0.2), p(0.0, 6.8)], stroke);
        }
    }
}
