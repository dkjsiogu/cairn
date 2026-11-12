//! 导出整合包：选要带上的东西，存成 `.mrpack`。网上能下到的 Mod、资源包、光影只记下载地址，
//! 别的打包进去。做完在同一个弹窗里告诉玩家结果，哪些没在网上找到。见 `core::export`。

use crate::prelude::*;
use nailong_core::export::{self, Item, Report};

#[derive(Default)]
pub(crate) struct ExportState {
    pub(crate) open: bool,
    /// 导出哪个版本（文件夹名）。
    version: String,
    /// 显示在标题下面的说明：`1.21.1 · NeoForge 21.1.230`。
    kind: String,
    name: String,
    pack_version: String,
    items: Vec<(Item, bool)>,
    /// 做完的结果；有它时弹窗显示结果。
    pub(crate) result: Option<Result<Report, String>>,
}

impl App {
    /// 打开导出弹窗：列出版本文件夹里能带上的东西，默认勾好。
    pub(crate) fn open_export(&mut self, row: &Row) {
        let items = export::items(&row.game_dir, &row.id)
            .into_iter()
            .map(|item| {
                let on = item.default_on;
                (item, on)
            })
            .collect();
        self.export = ExportState {
            open: true,
            version: row.id.clone(),
            kind: crate::pages::versions::row_kind(row),
            name: row.pack.clone().unwrap_or_else(|| row.id.clone()),
            pack_version: "1.0.0".into(),
            items,
            result: None,
        };
    }

    /// 选好存到哪，在后台导出。
    fn start_export(&mut self) {
        if self.busy.is_some() {
            self.export.result = Some(Err(self.busy_reason().into()));
            return;
        }
        let name = self.export.name.trim().to_string();
        let file = format!(
            "{}-{}.mrpack",
            versions::safe_name(if name.is_empty() {
                &self.export.version
            } else {
                &name
            }),
            self.export.pack_version.trim()
        );
        let Some(dest) = rfd::FileDialog::new()
            .set_title("导出整合包")
            .set_file_name(&file)
            .add_filter("Modrinth 整合包", &["mrpack"])
            .save_file()
        else {
            return;
        };
        let dest = if dest.extension().is_some_and(|e| e == "mrpack") {
            dest
        } else {
            dest.with_extension("mrpack")
        };
        let options = export::Options {
            name: if name.is_empty() {
                self.export.version.clone()
            } else {
                name
            },
            version: self.export.pack_version.trim().to_string(),
            summary: String::new(),
            include: self
                .export
                .items
                .iter()
                .filter(|(_, on)| *on)
                .map(|(item, _)| item.path.clone())
                .collect(),
        };
        let version = self.export.version.clone();
        self.export.open = false;
        cancel::reset();
        self.cancel_requested = false;
        self.busy = Some(Task::Export);
        self.activity = Some(
            Activity::new(options.name.clone(), "读取文件").expect(crate::activity::plan::EXPORT),
        );
        self.log.push(format!("导出整合包 {}", dest.display()));
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let progress = tx.clone();
        let sink: Sink = from_fn(move |p| {
            let _ = progress.send(Msg::Progress(p));
        });
        std::thread::spawn(move || {
            let result =
                export::export(&root, &version, &options, &dest, &sink).map_err(|e| e.to_string());
            let _ = tx.send(Msg::ExportDone(result));
        });
    }

    /// 后台导出做完：弹窗换成结果。
    pub(crate) fn finish_export(&mut self, result: Result<Report, String>) {
        self.busy = None;
        self.cancel_requested = false;
        cancel::reset();
        self.activity = None;
        self.rx = None;
        if matches!(&result, Err(e) if e.contains("取消")) {
            self.notice = Some("已取消导出".into());
            return;
        }
        self.export.result = Some(result);
        self.export.open = true;
    }

    pub(crate) fn export_overlay(&mut self, ctx: &egui::Context) {
        if !self.export.open {
            return;
        }
        let mut close = false;
        let mut start = false;
        let mut reveal: Option<PathBuf> = None;
        let modal = egui::Modal::new(egui::Id::unique("export_modal"))
            .backdrop_color(t::veil(1.0))
            .frame(crate::dialogs::account::modal_frame())
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(
                    egui::RichText::new("导出整合包")
                        .size(24.0)
                        .family(display_family()),
                );
                ui.label(
                    egui::RichText::new(format!("{} · {}", self.export.version, self.export.kind))
                        .size(13.0)
                        .color(t::sub()),
                );
                ui.add_space(14.0);
                match self.export.result.clone() {
                    Some(result) => {
                        self.export_result(ui, &result, &mut reveal);
                        ui.add_space(16.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if motion::primary_sized(ui, "完成", true, 86.0, 32.0, 12.0) {
                                close = true;
                            }
                        });
                    }
                    None => {
                        self.export_form(ui);
                        ui.add_space(16.0);
                        let any = self.export.items.iter().any(|(_, on)| *on);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if motion::primary_sized(ui, "导出…", any, 96.0, 32.0, 12.0) {
                                start = true;
                            }
                            ui.add_space(6.0);
                            if motion::secondary_sized(ui, "取消", true, 72.0, 32.0, 12.0).clicked()
                            {
                                close = true;
                            }
                        });
                    }
                }
            });
        if modal.backdrop_response.clicked() || escape_pressed(ctx) {
            close = true;
        }
        if let Some(path) = reveal {
            reveal_file(&path);
        }
        if start {
            self.start_export();
        }
        if close {
            self.export.open = false;
            self.export.result = None;
        }
    }

    /// 名字、版本号和要带上的东西。
    fn export_form(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("名字").size(13.0).color(t::sub()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.export.name)
                        .desired_width(340.0)
                        .frame(crate::dialogs::add_account::field_frame()),
                );
            });
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("版本号").size(13.0).color(t::sub()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.export.pack_version)
                        .desired_width(ui.available_width())
                        .frame(crate::dialogs::add_account::field_frame()),
                );
            });
        });
        ui.add_space(14.0);
        let mut all = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("要带上的东西")
                    .size(13.0)
                    .color(t::sub()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action_link(ui, "恢复默认").clicked() {
                    all = Some(None);
                }
                if action_link(ui, "全选").clicked() {
                    all = Some(Some(true));
                }
            });
        });
        if let Some(choice) = all {
            for (item, on) in &mut self.export.items {
                *on = choice.unwrap_or(item.default_on);
            }
        }
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(t::bg())
            .stroke(egui::Stroke::new(1.0, t::hairline()))
            .corner_radius(motion::R)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if self.export.items.is_empty() {
                            ui.label(
                                egui::RichText::new("这个版本文件夹里没有能导出的东西")
                                    .size(13.0)
                                    .color(t::dim()),
                            );
                        }
                        let mut shown_worlds = false;
                        for (item, on) in &mut self.export.items {
                            if item.world && !shown_worlds {
                                shown_worlds = true;
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("世界")
                                        .size(12.0)
                                        .family(bold_family())
                                        .color(t::sub()),
                                );
                            }
                            export_row(ui, item, on);
                        }
                    });
            });
        ui.add_space(6.0);
        let picked: u64 = self
            .export
            .items
            .iter()
            .filter(|(_, on)| *on)
            .map(|(item, _)| item.size)
            .sum();
        ui.label(
            egui::RichText::new(format!(
                "勾选了 {}。Mod、资源包、光影能在 Modrinth 或 CurseForge 下到的只记地址，不打包；其余打包进去",
                human_size(picked)
            ))
            .size(12.0)
            .color(t::dim()),
        );
    }

    fn export_result(
        &self,
        ui: &mut egui::Ui,
        result: &Result<Report, String>,
        reveal: &mut Option<PathBuf>,
    ) {
        let report = match result {
            Ok(report) => report,
            Err(e) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("没能导出：{e}"))
                            .size(13.0)
                            .color(t::danger()),
                    )
                    .wrap(),
                );
                return;
            }
        };
        ui.label(
            egui::RichText::new(format!("已导出，{}", human_size(report.bytes)))
                .size(15.0)
                .color(t::ok()),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "{} 个文件只记下载地址，安装时自己去下；{} 个文件打包进去",
                report.online, report.packed
            ))
            .size(13.0)
            .color(t::sub()),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let shown = elide_middle(
                ui,
                &report.path.display().to_string(),
                &egui::FontId::proportional(12.5),
                400.0,
            );
            ui.label(egui::RichText::new(shown).size(12.5).color(t::dim()))
                .on_hover_text(report.path.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action_link(ui, "在文件夹中显示").clicked() {
                    *reveal = Some(report.path.clone());
                }
            });
        });
        for warning in &report.warnings {
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(egui::RichText::new(warning).size(13.0).color(t::warn())).wrap(),
            );
        }
        if !report.not_found.is_empty() {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!(
                    "网上没找到、打包进去的（{} 个）",
                    report.not_found.len()
                ))
                .size(13.0)
                .color(t::sub()),
            );
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(t::bg())
                .stroke(egui::Stroke::new(1.0, t::hairline()))
                .corner_radius(motion::R)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for path in &report.not_found {
                                ui.label(egui::RichText::new(path).size(12.5));
                            }
                        });
                });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("自己做的、改过的，或者作者不让第三方下载的 Mod 会在这里")
                    .size(12.0)
                    .color(t::dim()),
            );
        }
    }
}

/// 一项：勾选框、名字、大小和文件数。整行都能点。
fn export_row(ui: &mut egui::Ui, item: &Item, on: &mut bool) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    list_row_fill(ui, rect, false, &response);
    if response.clicked() {
        *on = !*on;
    }
    let painter = ui.painter();
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::vec2(14.0, 14.0),
    );
    if *on {
        painter.rect_filled(box_rect, 3.0, t::accent());
        painter.line(
            vec![
                box_rect.left_center() + egui::vec2(3.0, 0.0),
                box_rect.center_bottom() + egui::vec2(-1.0, -3.5),
                box_rect.right_top() + egui::vec2(-3.0, 3.5),
            ],
            egui::Stroke::new(1.6, t::on_accent()),
        );
    } else {
        painter.rect_stroke(
            box_rect,
            3.0,
            egui::Stroke::new(1.0, t::control()),
            egui::StrokeKind::Inside,
        );
    }
    let name = item.path.strip_prefix("saves/").unwrap_or(&item.path);
    let label = if item.folder {
        format!("{name}/")
    } else {
        name.to_string()
    };
    painter.text(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        short_text(&label, 34),
        egui::FontId::proportional(13.0),
        if *on { t::ink() } else { t::sub() },
    );
    let detail = if item.folder {
        format!("{} · {} 个文件", human_size(item.size), item.files)
    } else {
        human_size(item.size)
    };
    painter.text(
        egui::pos2(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        detail,
        egui::FontId::proportional(12.0),
        t::dim(),
    );
}
