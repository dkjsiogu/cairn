//! 通用组件：按钮、胶囊、输入框、搜索框、路径行、列表行、标签，以及打开文件夹这类小工具。页面只拼装它们。

use crate::prelude::*;

/// 内容页的「已安装 / 发现」主模式：橙色底、深色字，一眼能看出当前在哪。
/// Four-block pixel spinner. Used anywhere the old egui arc spinner was;
/// it keeps the whole shell on the same grid and only requests frames while
/// it is actually visible.
pub(crate) fn busy_spinner(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let time = if motion::reduced() {
        0.0
    } else {
        ui.input(|i| i.time) as f32
    };
    ui.painter()
        .circle_stroke(rect.center(), 6.0, egui::Stroke::new(1.0, t::hairline()));
    let points: Vec<egui::Pos2> = (0..=12)
        .map(|index| {
            let angle = time * 4.0 + index as f32 / 12.0 * 1.8;
            rect.center() + egui::vec2(angle.cos(), angle.sin()) * 6.0
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(1.4, t::accent()),
    ));
    if !motion::reduced() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// 左侧激活态竖条：导航项、版本行、世界行、设置分栏与命令面板统一使用。
pub(crate) fn active_accent_bar(painter: &egui::Painter, rect: egui::Rect) {
    let h = (rect.height() - 14.0).clamp(12.0, 22.0);
    let bar = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 3.0, rect.center().y),
        egui::vec2(2.5, h),
    );
    painter.rect_filled(bar, 2.0, t::accent());
}

/// 键盘快捷键小键帽（如 `Ctrl+K`、`Esc`、`Enter`）。
pub(crate) fn kbd_badge(ui: &mut egui::Ui, text: &str) {
    let font = egui::FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t::dim());
    let w = (galley.size().x + 10.0).max(20.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 19.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        4,
        t::bg(),
        egui::Stroke::new(1.0, t::hairline()),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        t::dim(),
    );
}

/// 分段切换容器：凹陷轨道底色 + 细边，里面放 `content_tab`。
pub(crate) fn segmented_bar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(t::rail())
        .stroke(egui::Stroke::new(1.0, t::hairline()))
        .corner_radius(motion::R + 2)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                add(ui);
            });
        });
}

/// 分段控件里的一项：选中时浮起成 `surface` 胶囊带细边，可带数量徽章。
pub(crate) fn content_tab(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    active: bool,
) -> egui::Response {
    let font = egui::FontId::new(
        13.5,
        if active {
            bold_family()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let text_color = if active { t::ink() } else { t::sub() };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let badge_galley = (count > 0).then(|| {
        ui.painter().layout_no_wrap(
            count.to_string(),
            egui::FontId::proportional(11.5),
            if active { t::accent() } else { t::dim() },
        )
    });
    let badge_w = badge_galley
        .as_ref()
        .map_or(0.0, |g| (g.size().x + 10.0).max(20.0) + 6.0);
    let w = (galley.size().x + badge_w + 22.0).max(58.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
    let hover = motion::hover_amount(ui, response.id.with("seg_hover"), response.hovered());
    if active {
        ui.painter().rect(
            rect,
            motion::R,
            t::surface(),
            egui::Stroke::new(1.0, t::hairline()),
            egui::StrokeKind::Inside,
        );
    } else if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, motion::R, t::tint(t::wash(), 0.65 * hover));
    }
    let content_w = galley.size().x + badge_w;
    let x = rect.center().x - content_w * 0.5;
    let color = if active {
        t::ink()
    } else {
        motion::mix(t::sub(), t::ink(), hover)
    };
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    if let Some(bg) = badge_galley {
        let bw = (bg.size().x + 10.0).max(20.0);
        let br = egui::Rect::from_center_size(
            egui::pos2(x + content_w - bw * 0.5, rect.center().y),
            egui::vec2(bw, 17.0),
        );
        ui.painter().rect_filled(
            br,
            8,
            if active {
                t::tint(t::accent(), 0.16)
            } else {
                t::tint(t::wash(), 0.85)
            },
        );
        ui.painter().galley(
            egui::pos2(
                br.center().x - bg.size().x * 0.5,
                br.center().y - bg.size().y * 0.5,
            ),
            bg,
            if active { t::accent() } else { t::dim() },
        );
    }
    response
}

/// 下划线式次级标签（如 Mod / 资源包 / 光影），与分段主切换清晰区分。
pub(crate) fn underline_tab(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let font = egui::FontId::new(
        13.5,
        if active {
            bold_family()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, t::ink());
    let w = (galley.size().x + 20.0).max(52.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, 32.0), egui::Sense::click());
    let hover = motion::hover_amount(ui, response.id.with("utab_hover"), response.hovered());
    if !active && hover > 0.0 {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(0.0, 2.0)),
            motion::R,
            t::tint(t::wash(), 0.5 * hover),
        );
    }
    let color = if active {
        t::ink()
    } else {
        motion::mix(t::sub(), t::ink(), hover)
    };
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5 - 1.0,
        ),
        galley,
        color,
    );
    if active {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.bottom() - 2.5),
            egui::pos2(rect.right() - 8.0, rect.bottom() - 0.5),
        );
        ui.painter().rect_filled(bar, 1.5, t::accent());
    }
    response
}

/// 品类与选项胶囊：自定义绘制，高度和边距统一、选中带雾蓝轻底。
pub(crate) fn chip(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let font = egui::FontId::new(
        13.0,
        if selected {
            bold_family()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t::ink());
    let w = (galley.size().x + 22.0).max(40.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
    let hover = motion::hover_amount(ui, response.id.with("chip_hover"), response.hovered());
    let (fill, border, color) = if selected {
        (
            t::tint(t::accent(), 0.14),
            t::tint(t::accent(), 0.62),
            t::ink(),
        )
    } else if response.is_pointer_button_down_on() {
        (t::wash(), t::control(), t::ink())
    } else {
        (
            motion::mix(t::tint(t::surface(), 0.55), t::wash(), hover),
            motion::mix(t::hairline(), t::control(), hover),
            motion::mix(t::sub(), t::ink(), hover),
        )
    };
    ui.painter().rect(
        rect,
        motion::R,
        fill,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}

/// 下载量用中文单位，避免卡片里塞一长串数字。
/// 卡片上只留一小段介绍，完整描述在详情页。
pub(crate) fn short_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

pub(crate) fn compact(count: u64) -> String {
    if count >= 100_000_000 {
        format!("{:.1}亿", count as f64 / 100_000_000.0)
    } else if count >= 10_000 {
        format!("{:.1}万", count as f64 / 10_000.0)
    } else {
        count.to_string()
    }
}

pub(crate) fn quick_action(ui: &mut egui::Ui, text: &str) -> egui::Response {
    motion::secondary_sized(ui, text, true, 88.0, 30.0, 11.0)
}

pub(crate) fn action_link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t::sub());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 12.0, 26.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("link_hover"), response.hovered());
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, motion::R, t::tint(t::wash(), 0.7 * hover));
    }
    let color = motion::mix(t::sub(), t::ink(), hover);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}

pub(crate) fn status_dot(ui: &mut egui::Ui, ok: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 3.5, if ok { t::ok() } else { t::warn() });
}

pub(crate) fn inline_dot(ui: &mut egui::Ui) {
    ui.add_space(7.0);
    ui.label(egui::RichText::new("·").size(13.0).color(t::sub()));
    ui.add_space(7.0);
}

pub(crate) fn info_line(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(13.0).color(t::sub()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(13.0));
        });
    });
}

/// 列表行的背景：当前项和悬停项都填 `wash`，不加色条。
/// 列表行的背景：当前项填 `wash`；悬停时渐渐浅浅地填上，按下时再深一点，松手前就看得出点到了。
/// 右键菜单开着时也算悬停，看得出菜单是哪一行的。不加色条。
pub(crate) fn list_row_fill(
    ui: &egui::Ui,
    rect: egui::Rect,
    active: bool,
    response: &egui::Response,
) {
    let hovered = response.hovered() || response.context_menu_opened();
    let hover = motion::hover_amount(ui, response.id.with("row_hover"), hovered);
    let fill = if response.is_pointer_button_down_on() {
        motion::mix(t::wash(), t::control(), 0.35)
    } else if active {
        t::wash()
    } else if hover > 0.0 {
        t::tint(t::wash(), 0.7 * hover)
    } else {
        return;
    };
    ui.painter().rect_filled(rect, motion::R, fill);
}

pub(crate) fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(t::surface())
        .stroke(egui::Stroke::new(1.0, t::hairline()))
        .corner_radius(motion::R_CARD)
        .inner_margin(egui::Margin::same(18))
}

/// 加载骨架：不伪装成真实内容，只给一个稳定的占位高度。
pub(crate) fn skeleton_line(ui: &mut egui::Ui, height: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, height / 2.0, t::wash());
}

pub(crate) fn ghost_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t::sub());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 18.0).max(56.0), 28.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("ghost_hover"), response.hovered());
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, motion::R, t::tint(t::wash(), 0.8 * hover));
    }
    let color = motion::mix(t::sub(), t::ink(), hover);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}

pub(crate) fn open_folder(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// 在文件管理器里打开所在目录并选中这个文件；做不到时退回打开目录。
pub(crate) fn reveal_file(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // freedesktop 的 FileManager1 接口能选中文件（Dolphin、Nautilus、Nemo 都支持）。
        let uri = format!("file://{}", percent_path(path));
        let shown = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !shown && let Some(parent) = path.parent() {
            open_folder(parent);
        }
    }
}

/// 文件 URI 里的路径要百分号编码：中文目录名和空格都很常见。
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn percent_path(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub(crate) fn human_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let size = bytes as f64;
    if size >= KIB * KIB * KIB {
        format!("{:.2} GB", size / (KIB * KIB * KIB))
    } else if size >= KIB * KIB {
        format!("{:.1} MB", size / (KIB * KIB))
    } else if size >= KIB {
        format!("{:.0} KB", size / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn human_age(modified: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let diff = now.saturating_sub(modified);
    match diff {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{} 分钟前", diff / 60),
        3600..=86399 => format!("{} 小时前", diff / 3600),
        _ => format!("{} 天前", diff / 86400),
    }
}

pub(crate) fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|input| input.key_pressed(egui::Key::Escape))
}

pub(crate) fn progress_detail(p: &Progress) -> String {
    if p.bytes_total > 0 {
        format!("{} / {}", bytes(p.bytes_done), bytes(p.bytes_total))
    } else if p.files_total > 0 {
        format!("{} / {} 个文件", p.files_done, p.files_total)
    } else {
        String::new()
    }
}

pub(crate) fn bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{n:.0} B")
    }
}

pub(crate) fn primary_wide(ui: &mut egui::Ui, text: &str, enabled: bool, width: f32) -> bool {
    motion::primary_sized(ui, text, enabled, width, 40.0, 13.5)
}

pub(crate) fn accent_outline(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::new(13.0, bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, t::accent());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 24.0).max(86.0), 30.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("acc_hover"), response.hovered());
    let alpha = if response.is_pointer_button_down_on() {
        0.26
    } else {
        0.13 + 0.08 * hover
    };
    ui.painter().rect(
        rect,
        motion::R,
        t::tint(t::accent(), alpha),
        egui::Stroke::new(1.0, t::tint(t::accent(), 0.65 + 0.25 * hover)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        t::accent(),
    );
    response
}

pub(crate) fn text_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            ),
    );
    motion::mark_custom_focus(ui.ctx(), response.id);
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            motion::R,
            egui::Stroke::new(1.5, t::accent()),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// 按宽度从中间省略：路径的开头（盘符、用户目录）和结尾（具体是哪个版本）都留着。
pub(crate) fn elide_middle(
    ui: &egui::Ui,
    text: &str,
    font: &egui::FontId,
    max_width: f32,
) -> String {
    let width = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    if width(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let keep = (lo + hi).div_ceil(2);
        let head = keep * 2 / 5;
        let tail = keep - head;
        let candidate: String = chars[..head]
            .iter()
            .chain(std::iter::once(&'…'))
            .chain(&chars[chars.len() - tail..])
            .collect();
        if width(&candidate) <= max_width {
            lo = keep;
        } else {
            hi = keep - 1;
        }
    }
    let head = lo * 2 / 5;
    let tail = lo - head;
    chars[..head]
        .iter()
        .chain(std::iter::once(&'…'))
        .chain(&chars[chars.len() - tail..])
        .collect()
}

/// 「标签 · 路径 · 打开」一行：标签和按钮宽度固定，路径占剩下的地方，太长从中间省略；
/// 悬停看全路径，点一下复制。返回「打开」是否被点了。
pub(crate) fn path_row(ui: &mut egui::Ui, label: &str, path: &std::path::Path) -> bool {
    path_row_with(ui, label, path, None).0
}

/// 和 `path_row` 一样，最右边多一个按钮。返回（点了打开，点了这个按钮）。
pub(crate) fn path_row_with(
    ui: &mut egui::Ui,
    label: &str,
    path: &std::path::Path,
    extra: Option<&str>,
) -> (bool, bool) {
    let full = path.display().to_string();
    let font = egui::FontId::proportional(13.0);
    let mut open = false;
    let mut extra_clicked = false;
    let extra_w = if extra.is_some() { 54.0 + 6.0 } else { 0.0 };
    ui.horizontal(|ui| {
        let row_w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(64.0, 24.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(64.0);
                ui.label(egui::RichText::new(label).size(13.0).color(t::sub()));
            },
        );
        let path_w = (row_w - 64.0 - 54.0 - 16.0 - extra_w).max(60.0);
        let shown = elide_middle(ui, &full, &font, path_w);
        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(path_w, 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(path_w);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&shown)
                                .font(font.clone())
                                .color(t::sub()),
                        )
                        .sense(egui::Sense::click()),
                    )
                },
            )
            .inner;
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        }
        let copied_id = egui::Id::unique(("path_copied", label));
        if response.clicked() {
            ui.ctx().copy_text(full.clone());
            ui.ctx()
                .data_mut(|d| d.insert_temp(copied_id, ui.input(|i| i.time)));
        }
        let copied = ui
            .ctx()
            .data(|d| d.get_temp::<f64>(copied_id))
            .is_some_and(|at| ui.input(|i| i.time) - at < 1.5);
        response.on_hover_text(if copied {
            format!("已复制\n{full}")
        } else {
            format!("{full}\n点一下复制")
        });
        if motion::secondary_sized(ui, "打开", true, 54.0, 24.0, 10.0).clicked() {
            open = true;
        }
        if let Some(extra) = extra
            && motion::secondary_sized(ui, extra, true, 54.0, 24.0, 10.0).clicked()
        {
            extra_clicked = true;
        }
    });
    (open, extra_clicked)
}

/// 搜索框：左边放大镜（聚焦变雾蓝），自带聚焦高亮边，有内容时右边一个清除按钮。
pub(crate) fn search_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin {
                        left: 30,
                        right: 26,
                        top: 6,
                        bottom: 6,
                    }),
            ),
    );
    motion::mark_custom_focus(ui.ctx(), response.id);
    let rect = response.rect;
    let focused = response.has_focus();
    if focused {
        ui.painter().rect_stroke(
            rect,
            motion::R,
            egui::Stroke::new(1.5, t::accent()),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            motion::R,
            egui::Stroke::new(1.0, t::control()),
            egui::StrokeKind::Inside,
        );
    }
    let color = if focused { t::accent() } else { t::dim() };
    let c = egui::pos2(rect.left() + 14.5, rect.center().y - 1.0);
    let painter = ui.painter();
    painter.circle_stroke(c, 4.5, egui::Stroke::new(1.3, color));
    painter.line_segment(
        [c + egui::vec2(3.3, 3.3), c + egui::vec2(6.5, 6.5)],
        egui::Stroke::new(1.5, color),
    );
    if !text.is_empty() {
        let clear = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        if ui
            .put(
                clear,
                egui::Button::new(egui::RichText::new("×").size(14.0).color(t::dim())).frame(false),
            )
            .on_hover_text("清除（Esc）")
            .clicked()
        {
            text.clear();
        }
    }
    response
}

pub(crate) fn loader_label(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "fabric" => "Fabric".into(),
        "neoforge" => "NeoForge".into(),
        "forge" => "Forge".into(),
        "quilt" => "Quilt".into(),
        other => other.into(),
    }
}

/// Exact-size pill for toolbars and modal corners. Unlike `tag_fill` it
/// refuses to stretch to the height of the surrounding row.
pub(crate) fn tag_pill(ui: &mut egui::Ui, text: &str, color: egui::Color32, fill: egui::Color32) {
    let font = egui::FontId::new(13.0, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 14.0).max(30.0), 20.0),
        egui::Sense::hover(),
    );
    ui.painter().rect(
        rect,
        motion::R,
        fill,
        egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 46),
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
}

pub(crate) fn tag(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    tag_fill(ui, text, color, t::wash());
}

pub(crate) fn tag_fill(ui: &mut egui::Ui, text: &str, color: egui::Color32, fill: egui::Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(motion::R)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(13.0).color(color));
        });
}

pub(crate) fn small_accent(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::new(12.5, bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, t::accent());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 20.0).max(46.0), 26.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("sacc_hover"), response.hovered());
    ui.painter().rect(
        rect,
        motion::R,
        t::tint(t::accent(), 0.13 + 0.09 * hover),
        egui::Stroke::new(1.0, t::tint(t::accent(), 0.6 + 0.25 * hover)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        t::accent(),
    );
    response
}

pub(crate) fn tiny_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t::sub());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 18.0).max(46.0), 26.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("tiny_hover"), response.hovered());
    ui.painter().rect(
        rect,
        motion::R,
        motion::mix(t::tint(t::bg(), 0.65), t::wash(), hover),
        egui::Stroke::new(1.0, motion::mix(t::hairline(), t::control(), hover)),
        egui::StrokeKind::Inside,
    );
    let color = motion::mix(t::sub(), t::ink(), hover);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}

pub(crate) fn danger_tiny(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::new(12.5, bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, t::danger());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2((galley.size().x + 20.0).max(64.0), 26.0),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("dtiny_hover"), response.hovered());
    ui.painter().rect(
        rect,
        motion::R,
        t::tint(t::danger(), 0.14 + 0.10 * hover),
        egui::Stroke::new(1.0, t::tint(t::danger(), 0.65)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        t::danger(),
    );
    response
}

/// 危险操作的确认按钮：`danger` 色的字和细边，淡淡的底。只在二次确认时出现。
pub(crate) fn danger_button(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    height: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height.max(24.0)), egui::Sense::click());
    let hover = motion::hover_amount(ui, response.id.with("hover"), response.hovered());
    let fill = if response.is_pointer_button_down_on() {
        0.30
    } else {
        0.14 + 0.08 * hover
    };
    ui.painter().rect(
        rect,
        motion::R,
        t::tint(t::danger(), fill),
        egui::Stroke::new(1.0, t::tint(t::danger(), 0.7)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(14.0),
        t::danger(),
    );
    response
}

/// 统一的浮层弹窗框架：`R_MODAL` 圆角 + 柔和投影 + 细边。
pub(crate) fn elevated_modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(t::surface())
        .stroke(egui::Stroke::new(1.0, t::hairline()))
        .corner_radius(motion::R_MODAL)
        .shadow(t::modal_shadow())
        .inner_margin(egui::Margin::symmetric(26, 22))
}

/// 弹窗的外框：遮罩、`surface` 底、细边与柔和投影。返回（内容的返回值，点了遮罩或按了 Esc）。
/// 新弹窗都用它，外观和关闭方式就一致了；别忘了把开关加进 `App::modal_open`。
pub(crate) fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let modal = egui::Modal::new(egui::Id::unique(("dialog", id)))
        .backdrop_color(t::veil(1.0))
        .frame(elevated_modal_frame())
        .show(ctx, |ui| {
            ui.set_width(width);
            add(ui)
        });
    let dismissed = modal.backdrop_response.clicked() || escape_pressed(ctx);
    (modal.inner, dismissed)
}

/// 弹窗的标题和下面一行说明。
pub(crate) fn dialog_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(24.0)
            .family(display_family())
            .color(t::ink()),
    );
    if !subtitle.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(subtitle).size(13.0).color(t::sub())).wrap());
    }
}

pub(crate) fn short_date(text: &str) -> String {
    text.get(..10).unwrap_or(text).to_string()
}
