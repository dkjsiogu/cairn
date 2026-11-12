//! 复制版本：想试新 Mod 又怕弄坏时，先复制一份。Mod、配置、资源包照搬，存档可选。

use crate::prelude::*;

pub(crate) struct Duplicate {
    from: String,
    name: String,
    worlds: bool,
    /// 这个版本有几个世界；没有就不问带不带。
    world_count: usize,
    rx: Option<mpsc::Receiver<DuplicateMsg>>,
    progress: Option<Progress>,
    error: Option<String>,
}

enum DuplicateMsg {
    Progress(Progress),
    Done(Result<String, String>),
}

impl App {
    pub(crate) fn open_duplicate(&mut self, row: &Row) {
        let base = format!("{} 副本", row.id);
        self.duplicate = Some(Duplicate {
            from: row.id.clone(),
            name: versions::unique_name(&self.root, &base),
            worlds: false,
            world_count: saves::worlds(&row.game_dir).map_or(0, |w| w.len()),
            rx: None,
            progress: None,
            error: None,
        });
    }

    /// 每帧收复制的进度和结果。
    pub(crate) fn poll_duplicate(&mut self) {
        let Some(dup) = &mut self.duplicate else {
            return;
        };
        let Some(rx) = &dup.rx else {
            return;
        };
        let mut done = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                DuplicateMsg::Progress(p) => dup.progress = Some(p),
                DuplicateMsg::Done(result) => done = Some(result),
            }
        }
        let Some(result) = done else {
            self.ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        dup.rx = None;
        dup.progress = None;
        self.busy = None;
        self.activity = None;
        cancel::reset();
        match result {
            Ok(name) => {
                self.duplicate = None;
                self.refresh();
                self.selected = Some(name.clone());
                self.notice = Some(format!("已复制为「{name}」"));
                self.log.push(format!("复制版本 → {name}"));
            }
            Err(e) if e.contains("取消") => {
                dup.error = Some("已取消，没有留下文件".into());
            }
            Err(e) => dup.error = Some(e),
        }
    }

    fn start_duplicate(&mut self) {
        let Some(dup) = &mut self.duplicate else {
            return;
        };
        let (from, name, worlds) = (dup.from.clone(), dup.name.trim().to_string(), dup.worlds);
        let (tx, rx) = mpsc::channel();
        dup.rx = Some(rx);
        dup.error = None;
        self.busy = Some(Task::Files);
        cancel::reset();
        let root = self.root.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let progress = tx.clone();
            let repaint = ctx.clone();
            let sink = from_fn(move |p| {
                let _ = progress.send(DuplicateMsg::Progress(p));
                repaint.request_repaint();
            });
            let result =
                versions::duplicate(&root, &from, &name, worlds, &sink).map_err(|e| e.to_string());
            let _ = tx.send(DuplicateMsg::Done(result));
            ctx.request_repaint();
        });
    }

    pub(crate) fn duplicate_overlay(&mut self, ctx: &egui::Context) {
        let busy_block = self.write_block(None).filter(|_| self.busy.is_some());
        let Some(dup) = &mut self.duplicate else {
            return;
        };
        let working = dup.rx.is_some();
        let problem = match versions::check_name(&dup.name) {
            Err(e) => Some(e.to_string()),
            Ok(name) if versions::unique_name(&self.root, &name) != name => {
                Some("已经有叫这个名字的版本了".to_string())
            }
            Ok(_) => None,
        };
        let blocked = if working { None } else { busy_block };
        let mut start = false;
        let mut cancel_copy = false;
        let mut close = false;
        let from = dup.from.clone();
        let ((), dismissed) = dialog(ctx, "duplicate", 420.0, |ui| {
            dialog_title(ui, "复制版本", &format!("从「{from}」复制一份，原来的不动"));
            ui.add_space(14.0);
            ui.label(egui::RichText::new("新名字").size(13.0).color(t::sub()));
            ui.add_space(4.0);
            let field = ui.add_enabled(
                !working,
                egui::TextEdit::singleline(&mut dup.name)
                    .desired_width(ui.available_width())
                    .frame(crate::dialogs::add_account::field_frame()),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                start = true;
            }
            if let Some(problem) = &problem {
                ui.label(egui::RichText::new(problem).size(13.0).color(t::warn()));
            }
            ui.add_space(10.0);
            if dup.world_count > 0 {
                let label = format!("带上 {} 个世界", dup.world_count);
                if crate::dialogs::settings::toggle_row(
                    ui,
                    &label,
                    if dup.worlds {
                        "两边各玩各的"
                    } else {
                        "新版本里没有存档"
                    },
                    dup.worlds,
                ) && !working
                {
                    dup.worlds = !dup.worlds;
                }
                ui.add_space(6.0);
            }
            ui.label(
                egui::RichText::new("Mod、配置、资源包和启动参数都带过去；日志、备份和恢复点不带")
                    .size(12.5)
                    .color(t::dim()),
            );
            if let Some(p) = &dup.progress {
                ui.add_space(12.0);
                let time = ui.input(|i| i.time);
                motion::progress_bar(ui, p.fraction(), false, time);
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        progress_detail(p),
                        p.current.as_deref().unwrap_or("")
                    ))
                    .size(12.5)
                    .color(t::dim()),
                );
            }
            if let Some(error) = &dup.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error).size(13.0).color(t::danger()));
            }
            if let Some(reason) = blocked {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(reason).size(13.0).color(t::warn()));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if working {
                        if motion::secondary_sized(ui, "停止", true, 72.0, 34.0, 13.0).clicked() {
                            cancel_copy = true;
                        }
                    } else {
                        let ok = problem.is_none() && blocked.is_none();
                        if motion::primary_sized(ui, "复制", ok, 96.0, 34.0, 13.0) {
                            start = true;
                        }
                        if motion::secondary_sized(ui, "取消", true, 72.0, 34.0, 13.0).clicked() {
                            close = true;
                        }
                    }
                });
            });
        });
        let ready = problem.is_none() && blocked.is_none() && !working;
        if cancel_copy {
            cancel::cancel();
        }
        if (dismissed || close) && !working {
            self.duplicate = None;
        } else if start && ready {
            self.start_duplicate();
        }
    }
}
