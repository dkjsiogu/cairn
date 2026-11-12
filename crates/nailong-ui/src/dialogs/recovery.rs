//! 崩溃后的退路：上次正常游戏之后改过内容，就能一步退回改之前。
//!
//! 「坏了能救」的那一下：游戏异常退出时，底部提示多一个「退回改动」，点了先看清楚要退回什么，
//! 确认后用改动前自动留的那个恢复点。退回之前会再留一份现在的，退错了还能回来。

use crate::prelude::*;

impl App {
    pub(crate) fn recovery_overlay(&mut self, ctx: &egui::Context) {
        if !self.play.recovery_open {
            return;
        }
        let Some(recovery) = self.play.recovery.clone() else {
            self.play.recovery_open = false;
            return;
        };
        let blocked = self.write_block(Some(&recovery.row_id));
        let mut restore = false;
        let mut close = false;
        let mut show_points = false;
        let ((), dismissed) = dialog(ctx, "recovery", 440.0, |ui| {
            dialog_title(
                ui,
                "退回改动",
                &format!(
                    "「{}」上次正常玩是 {}",
                    recovery.row_id,
                    human_age(recovery.last_good)
                ),
            );
            ui.add_space(14.0);
            settings_card(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("那之后改过 {} 次内容", recovery.changes))
                        .size(14.0)
                        .family(bold_family()),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!(
                        "退回到「{}」之前（{}）",
                        recovery.point.reason,
                        human_age(recovery.point.created_at)
                    ))
                    .size(13.0)
                    .color(t::sub()),
                );
            });
            ui.add_space(10.0);
            for line in [
                "Mod、配置和加载器回到那时的样子",
                "世界存档不动",
                "退回前会先留一份现在的，退错了还能回来",
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("·").size(13.0).color(t::dim()));
                    ui.label(egui::RichText::new(line).size(13.0));
                });
            }
            if let Some(reason) = blocked {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(reason).size(13.0).color(t::warn()));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if action_link(ui, "看全部恢复点").clicked() {
                    show_points = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if motion::primary_sized(ui, "退回", blocked.is_none(), 96.0, 34.0, 13.0) {
                        restore = true;
                    }
                    if motion::secondary_sized(ui, "取消", true, 72.0, 34.0, 13.0).clicked() {
                        close = true;
                    }
                });
            });
        });
        if dismissed || close {
            self.play.recovery_open = false;
        }
        if show_points {
            self.play.recovery_open = false;
            if let Some(row) = self.row(&recovery.row_id) {
                self.selected = Some(row.id.clone());
                self.open_content(true);
                self.open_rollback();
            }
        }
        if restore && blocked.is_none() {
            self.play.recovery_open = false;
            self.play.recovery = None;
            self.error = None;
            self.notice = Some("正在退回…".into());
            let dir = recovery.game_dir.clone();
            let id = recovery.point.id.clone();
            let (tx, rx) = mpsc::channel();
            self.rollback.rx = Some(rx);
            self.rollback.busy = true;
            self.content.game_dir.get_or_insert(dir.clone());
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = rollback::restore(&dir, &id)
                    .map(|count| format!("已退回 {count} 个文件，可以再试试开始游戏"))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::RollbackDone(result));
                ctx.request_repaint();
            });
        }
    }
}
