//! 换数据目录：用 PCL、HMCL、官方启动器的 .minecraft，或者随便选一个文件夹。
//! 账户和设置一起带过去，换好后 Cairn 重新打开。见 `docs/design/27-version-folders.md` §10。

use crate::prelude::*;
use nailong_core::install::reuse::{self, OtherRoot};
use std::path::Path;

#[derive(Default)]
pub(crate) struct DataDirState {
    pub(crate) open: bool,
    found: Vec<OtherRoot>,
    picked: Option<PathBuf>,
    error: Option<String>,
}

impl App {
    pub(crate) fn open_data_dir(&mut self) {
        self.data_dir = DataDirState {
            open: true,
            found: reuse::other_roots(&self.root),
            picked: None,
            error: None,
        };
    }

    pub(crate) fn data_dir_overlay(&mut self, ctx: &egui::Context) {
        if !self.data_dir.open {
            return;
        }
        let mut close = false;
        let mut pick_folder = false;
        let mut switch: Option<Option<PathBuf>> = None;
        let chosen = nailong_core::paths::chosen_root();
        let modal = egui::Modal::new(egui::Id::unique("data_dir_modal"))
            .backdrop_color(t::veil(1.0))
            .frame(crate::dialogs::account::modal_frame())
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(
                    egui::RichText::new("换数据目录")
                        .size(24.0)
                        .family(display_family()),
                );
                ui.label(
                    egui::RichText::new(
                        "版本、运行库和资源都从这里读。可以直接用别的启动器的 .minecraft",
                    )
                    .size(13.0)
                    .color(t::sub()),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("现在").size(13.0).color(t::sub()));
                    ui.add_space(6.0);
                    let shown = elide_middle(
                        ui,
                        &self.root.dir().display().to_string(),
                        &egui::FontId::proportional(13.0),
                        440.0,
                    );
                    ui.label(egui::RichText::new(shown).size(13.0));
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("在这台电脑上找到的")
                        .size(13.0)
                        .color(t::sub()),
                );
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(t::bg())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin::symmetric(6, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                if self.data_dir.found.is_empty() {
                                    ui.label(
                                        egui::RichText::new(
                                            "没找到别的启动器的 .minecraft，可以自己选文件夹",
                                        )
                                        .size(13.0)
                                        .color(t::dim()),
                                    );
                                }
                                for other in &self.data_dir.found {
                                    let active = self.data_dir.picked.as_ref() == Some(&other.dir);
                                    if found_row(ui, other, active).clicked() {
                                        self.data_dir.picked = Some(other.dir.clone());
                                    }
                                }
                                // 自己选的文件夹也列出来，看得见选中了什么。
                                if let Some(picked) = &self.data_dir.picked
                                    && !self.data_dir.found.iter().any(|o| &o.dir == picked)
                                {
                                    let custom = OtherRoot {
                                        dir: picked.clone(),
                                        launcher: "自己选的".into(),
                                        versions: 0,
                                    };
                                    let _ = found_row(ui, &custom, true);
                                }
                            });
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if action_link(ui, "选择文件夹…").clicked() {
                        pick_folder = true;
                    }
                    if chosen.is_some() && action_link(ui, "回到默认位置").clicked() {
                        switch = Some(None);
                    }
                });
                if let Some(error) = &self.data_dir.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).size(13.0).color(t::danger()));
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("账户和设置会一起带过去。换好后 Cairn 会重新打开")
                        .size(12.0)
                        .color(t::dim()),
                );
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = self.data_dir.picked.is_some() && self.busy.is_none();
                    if motion::primary_sized(ui, "换过去", ready, 96.0, 32.0, 12.0) {
                        switch = Some(self.data_dir.picked.clone());
                    }
                    ui.add_space(6.0);
                    if motion::secondary_sized(ui, "取消", true, 72.0, 32.0, 12.0).clicked() {
                        close = true;
                    }
                });
            });
        if modal.backdrop_response.clicked() || escape_pressed(ctx) {
            close = true;
        }
        if pick_folder
            && let Some(dir) = rfd::FileDialog::new()
                .set_title("选择 .minecraft 文件夹")
                .pick_folder()
        {
            self.data_dir.picked = Some(dir);
            self.data_dir.error = None;
        }
        if let Some(target) = switch {
            if let Some(reason) = self.write_block(None) {
                self.data_dir.error = Some(reason.into());
            } else {
                match self.switch_data_dir(target.as_deref()) {
                    Ok(()) => std::process::exit(0),
                    Err(e) => self.data_dir.error = Some(e),
                }
            }
        }
        if close {
            self.data_dir.open = false;
        }
    }

    /// 带上账户和设置，记下新的数据目录，重新打开。`None` 是回到默认位置。
    fn switch_data_dir(&mut self, target: Option<&Path>) -> Result<(), String> {
        let dest = target
            .map(Path::to_path_buf)
            .unwrap_or_else(Root::default_root);
        self.save_ui_config();
        nailong_core::paths::carry_settings(&self.root, &dest).map_err(|e| e.to_string())?;
        nailong_core::paths::choose_root(target).map_err(|e| e.to_string())?;
        nailong_core::update::restart_fresh().map_err(|e| e.to_string())
    }
}

/// 找到的一个 .minecraft：哪个启动器的、几个版本、在哪。整行能点。
fn found_row(ui: &mut egui::Ui, other: &OtherRoot, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    list_row_fill(ui, rect, active, &response);
    let inner = rect.shrink2(egui::vec2(10.0, 6.0));
    let title = if other.versions > 0 {
        format!("{} · {} 个版本", other.launcher, other.versions)
    } else {
        other.launcher.clone()
    };
    ui.painter().text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(13.5),
        if active { t::ink() } else { t::sub() },
    );
    let path = elide_middle(
        ui,
        &other.dir.display().to_string(),
        &egui::FontId::proportional(12.0),
        inner.width(),
    );
    ui.painter().text(
        inner.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        path,
        egui::FontId::proportional(12.0),
        t::dim(),
    );
    response.on_hover_text(other.dir.display().to_string())
}
