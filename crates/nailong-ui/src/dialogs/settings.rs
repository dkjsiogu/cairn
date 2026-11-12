//! 设置弹窗：分四栏——游戏（账户、Java、内存和回收器）、启动器（外观、缩放、更新、下载源）、
//! 存储（目录和清理）、关于（版本、日志、快捷键）。

use crate::prelude::*;

impl App {
    pub(crate) fn start_bench(&mut self) {
        if self.busy.is_some() {
            return;
        }
        // 游戏开着时测出来的数不准。
        if self.any_game() {
            self.error = Some("游戏运行中，退出游戏后再测".into());
            return;
        }
        self.error = None;
        self.notice = None;
        self.busy = Some(Task::Bench);
        self.cancel_requested = false;
        cancel::reset();
        self.activity = Some(Activity::new("这台电脑", "准备测试"));
        self.log.push("测一下这台电脑".into());
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let progress = tx.clone();
            let sink = from_fn(move |p| {
                let _ = progress.send(Msg::Progress(p));
            });
            let msg = match bench::run(&root, &sink) {
                Ok(store) => Msg::BenchDone(Ok(store)),
                Err(Error::Cancelled) => Msg::BenchCancelled,
                Err(e) => Msg::BenchDone(Err(e.to_string())),
            };
            let _ = tx.send(msg);
        });
    }

    pub(crate) fn finish_bench(&mut self) {
        self.busy = None;
        self.rx = None;
        self.activity = None;
        self.cancel_requested = false;
        cancel::reset();
        self.play.tuning_cache.clear();
    }

    /// 开始测试前说清楚会发生什么。
    pub(crate) fn bench_overlay(&mut self, ctx: &egui::Context) {
        if !self.bench_confirm {
            return;
        }
        let (picked, _) = bench::candidates(&self.env.javas);
        let can_run = !picked.is_empty();
        let mut start = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("bench_modal"))
            .backdrop_color(t::veil(1.0))
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin::symmetric(26, 22)),
            )
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(
                    egui::RichText::new("测一下这台电脑")
                        .size(24.0)
                        .family(display_family()),
                );
                ui.label(
                    egui::RichText::new(if can_run {
                        format!(
                            "用 {} 比较 G1 和 ZGC，结论所有版本共用",
                            picked
                                .iter()
                                .map(|java| format!("Java {}", java.major))
                                .collect::<Vec<_>>()
                                .join("、")
                        )
                    } else {
                        "这台电脑没有 64 位的 Java 21 或更新版本：只能用 G1，不需要测试".into()
                    })
                    .size(13.0)
                    .color(if can_run { t::sub() } else { t::warn() }),
                );
                ui.add_space(14.0);
                let minutes = (picked.len() as f32 * 1.4).ceil().max(1.0) as u32;
                for line in [
                    "不开游戏，也不碰任何版本：后台跑一个模拟游戏负载的小程序".to_string(),
                    "负载照游戏来：常驻数据、区块缓存、生成线程、服务端和不封顶的渲染".to_string(),
                    "比平均帧率、1% 低帧、能看出来的顿和 CPU，分常规和大型整合包两档".to_string(),
                    format!("大约 {minutes} 分钟，期间电脑会比较忙，可以随时停止"),
                    "内存大小不在这里测：正常玩一局，启动器会从游戏里学".to_string(),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("·").size(13.0).color(t::dim()));
                        ui.label(egui::RichText::new(line).size(13.0));
                    });
                }
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if motion::primary_sized(ui, "开始测试", can_run, 120.0, 34.0, 13.0) {
                        start = true;
                    }
                    ui.add_space(8.0);
                    if motion::secondary_sized(ui, "取消", true, 72.0, 34.0, 13.0).clicked() {
                        close = true;
                    }
                });
            });
        if modal.backdrop_response.clicked() || escape_pressed(ctx) {
            close = true;
        }
        if start {
            self.bench_confirm = false;
            self.start_bench();
        } else if close {
            self.bench_confirm = false;
        }
    }

    /// 设置里的性能卡片：内存、回收器，以及当前版本会用的参数和计算过程。
    pub(crate) fn tuning_card(&mut self, ui: &mut egui::Ui) {
        let row = self.selected_row();
        let plan = row.as_ref().and_then(|row| self.tuning_for(row));
        egui::Frame::new()
            .fill(t::bg())
            .stroke(egui::Stroke::new(1.0, t::hairline()))
            .corner_radius(motion::R)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("内存").size(13.0).color(t::sub()));
                    let auto_mb = plan.as_ref().map_or(4096, |plan| {
                        if plan.auto_memory { plan.heap_mb } else { 4096 }
                    });
                    memory_stepper(ui, &mut self.prefs.memory, auto_mb);
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("回收器").size(13.0).color(t::sub()));
                    for gc in tuning::GcChoice::ALL {
                        if chip(ui, gc.label(), self.prefs.gc == gc).clicked() {
                            self.prefs.gc = gc;
                        }
                    }
                });
                let (Some(row), Some(plan)) = (row, plan) else {
                    return;
                };
                let entry = self
                    .tuning_entry(&row.id)
                    .map(|(work, java, learned)| (work.clone(), java.clone(), learned.clone()));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(plan.headline()).size(14.0));
                    // 版本名占剩下的地方，太长就省略，不和右边的链接重叠。
                    let room = (ui.available_width() - 72.0).max(40.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(room, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&row.label).size(13.0).color(t::dim()),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&row.label);
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.show_tuning {
                            "收起"
                        } else {
                            "查看计算"
                        };
                        if action_link(ui, label).clicked() {
                            self.show_tuning = !self.show_tuning;
                        }
                    });
                });
                ui.label(egui::RichText::new(&plan.basis).size(13.0).color(t::dim()));
                // 这里是全局默认；这个版本自己改过的项以启动参数为准，说清楚免得以为没生效。
                let cfg = self.version_cfg(&row.id);
                if !cfg.is_default() {
                    let mut own = Vec::new();
                    if cfg.java.is_some() {
                        own.push("Java");
                    }
                    if cfg.memory.is_some() {
                        own.push("内存");
                    }
                    if cfg.gc.is_some() {
                        own.push("回收器");
                    }
                    if cfg.jvm_args.is_some() {
                        own.push("JVM 参数");
                    }
                    if cfg.window.is_some() {
                        own.push("窗口");
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "这个版本单独设置了{}，以启动参数为准",
                                own.join("、")
                            ))
                            .size(13.0)
                            .color(t::accent()),
                        );
                        if action_link(ui, "去看看").clicked() {
                            self.show_settings = false;
                            self.open_launch_edit(&row);
                        }
                    });
                }
                for warning in &plan.warnings {
                    ui.label(egui::RichText::new(warning).size(13.0).color(t::warn()));
                }
                // 内存从哪来：玩过就按实测，没玩过提示一句。
                if plan.auto_memory
                    && entry
                        .as_ref()
                        .is_none_or(|(_, _, learned)| learned.as_ref().is_none_or(|l| l.live_mb.is_none()))
                {
                    ui.label(
                        egui::RichText::new("内存现在按公式估算；正常玩一局后改按游戏里的实测")
                            .size(13.0)
                            .color(t::dim()),
                    );
                }
                // 回收器从哪来：这台电脑测过没有。
                let java = entry.as_ref().map(|(_, java, _)| java.clone());
                let (text, color, action) = self.bench_status(java.as_ref());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).size(13.0).color(color))
                            .truncate(),
                    );
                    if let Some(action) = action {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if action_link(ui, action).clicked() {
                                self.bench_confirm = true;
                            }
                        });
                    }
                });
                if !self.show_tuning {
                    if !self.prefs.jvm_args.trim().is_empty() {
                        ui.label(
                            egui::RichText::new("另有自定义 JVM 参数")
                                .size(13.0)
                                .color(t::dim()),
                        );
                    }
                    return;
                }
                ui.add_space(8.0);
                for step in &plan.steps {
                    ui.label(egui::RichText::new(step).size(13.0).color(t::sub()));
                }
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(plan.args().join(" "))
                            .size(13.0)
                            .monospace()
                            .color(t::sub()),
                    )
                    .wrap(),
                );
                if let Some(store) = &self.env.bench {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "这台电脑的测试（{}）",
                            bench::age(store.when)
                        ))
                        .size(13.0)
                        .color(t::sub()),
                    );
                    for result in &store.javas {
                        for tier in &result.tiers {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Java {} · {}档 → {}",
                                    result.major,
                                    tier.label,
                                    tier.collector.label()
                                ))
                                .size(13.0)
                                .color(t::sub()),
                            );
                            for run in [&tier.g1, &tier.zgc] {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "    {} {}：平均 {:.0} 帧 · 1% 低 {:.0} 帧 · 每分钟顿 {:.0} 次{}",
                                        run.collector.label(),
                                        tuning::gb(run.heap_mb),
                                        run.fps,
                                        run.low_fps,
                                        run.hitches_per_min,
                                        run.cpu_cores
                                            .map(|c| format!(" · CPU {c:.1} 核"))
                                            .unwrap_or_default()
                                    ))
                                    .size(13.0)
                                    .color(t::dim()),
                                );
                            }
                        }
                        for note in &result.notes {
                            ui.label(egui::RichText::new(note).size(13.0).color(t::dim()));
                        }
                    }
                    for note in &store.skipped {
                        ui.label(
                            egui::RichText::new(format!("没测：{note}"))
                                .size(13.0)
                                .color(t::dim()),
                        );
                    }
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("额外 JVM 参数")
                        .size(13.0)
                        .color(t::sub()),
                );
                text_field(
                    ui,
                    &mut self.prefs.jvm_args,
                    "和自动参数冲突时以这里为准",
                    ui.available_width(),
                );
            });
    }

    /// 回收器那一行：测过没有、结论是什么、能不能再测。
    pub(crate) fn bench_status(
        &self,
        java: Option<&java::JavaRuntime>,
    ) -> (String, egui::Color32, Option<&'static str>) {
        let testable = !bench::candidates(&self.env.javas).0.is_empty();
        let Some(store) = &self.env.bench else {
            return if testable {
                (
                    "回收器按经验选；测一下这台电脑更准，约 1–2 分钟".into(),
                    t::dim(),
                    Some("测一下"),
                )
            } else {
                ("没有 Java 21+，回收器只能用 G1".into(), t::dim(), None)
            };
        };
        if !store.fresh() {
            return (
                "上次测试后电脑的核数或内存变了，结论不再使用".into(),
                t::warn(),
                testable.then_some("重新测试"),
            );
        }
        let age = bench::age(store.when);
        match java {
            Some(java) if java.major < 21 => (
                format!("这个版本用 Java {}，只用 G1，不需要测试", java.major),
                t::dim(),
                testable.then_some("重新测试"),
            ),
            Some(java) if java.is_graal() => (
                "这个版本用 GraalVM，只用 G1，不需要测试".into(),
                t::dim(),
                testable.then_some("重新测试"),
            ),
            Some(java) => match store.find(java) {
                Some(result) => {
                    let tiers = result
                        .tiers
                        .iter()
                        .map(|tier| format!("{}档 {}", tier.label, tier.collector.label()))
                        .collect::<Vec<_>>()
                        .join("、");
                    (
                        format!("{age}测过这台电脑：Java {} {tiers}", result.major),
                        t::dim(),
                        Some("重新测试"),
                    )
                }
                None => (
                    format!("{age}的测试里没有 Java {}，回收器按经验选", java.major),
                    t::dim(),
                    Some("重新测试"),
                ),
            },
            None => (format!("{age}测过这台电脑"), t::dim(), Some("重新测试")),
        }
    }

    /// 设置不是一级页面，只在这里出现。左边分栏，右边是这一栏的内容；改了就生效，关掉时存盘。
    pub(crate) fn settings_overlay(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut actions = SettingsActions::default();
        let screen = ctx.content_rect();
        let width = (screen.width() - 64.0).clamp(560.0, 720.0);
        let body_h = (screen.height() - 170.0).clamp(300.0, 470.0);
        let nav_w = 148.0;
        let modal = egui::Modal::new(egui::Id::unique("settings_modal"))
            .backdrop_color(t::veil(1.0))
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(22, 20)),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.horizontal_top(|ui| {
                    let spacing = ui.spacing().item_spacing.x;
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // 左边：标题和分栏。
                    ui.allocate_ui_with_layout(
                        egui::vec2(nav_w, body_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.set_width(nav_w);
                            ui.set_min_height(body_h);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new("设置")
                                        .size(24.0)
                                        .family(display_family()),
                                );
                            });
                            ui.add_space(14.0);
                            for tab in SettingsTab::ALL {
                                let (rect, response, color) =
                                    crate::pages::nav_row(ui, 34.0, self.settings_tab == tab, true);
                                ui.painter().text(
                                    egui::pos2(rect.left() + 12.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    tab.label(),
                                    egui::FontId::proportional(14.0),
                                    color,
                                );
                                if response.on_hover_text(tab.hint()).clicked() {
                                    self.settings_tab = tab;
                                }
                            }
                        },
                    );
                    ui.add_space(16.0);
                    let (line, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, body_h), egui::Sense::hover());
                    ui.painter().rect_filled(line, 0.0, t::hairline());
                    ui.add_space(20.0);
                    // 右边：这一栏的内容。
                    let body_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(body_w, body_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.set_width(body_w);
                            ui.spacing_mut().item_spacing.x = spacing;
                            egui::ScrollArea::vertical()
                                .id_salt(("settings_body", self.settings_tab as u8))
                                .max_height(body_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // 给滚动条让出位置，卡片的右边不被压住。
                                    ui.set_width(ui.available_width() - 12.0);
                                    match self.settings_tab {
                                        SettingsTab::Game => self.settings_game(ui, &mut actions),
                                        SettingsTab::Launcher => self.settings_launcher(ui),
                                        SettingsTab::Storage => {
                                            self.settings_storage(ui, &mut actions)
                                        }
                                        SettingsTab::About => self.settings_about(ui, &mut actions),
                                    }
                                    ui.add_space(4.0);
                                });
                        },
                    );
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("改了就生效").size(13.0).color(t::dim()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if motion::primary_sized(ui, "完成", true, 86.0, 32.0, 12.0) {
                            actions.close = true;
                        }
                    });
                });
            });
        if modal.backdrop_response.clicked() || escape_pressed(ctx) {
            actions.close = true;
        }
        if actions.open_account {
            self.save_ui_config();
            self.show_settings = false;
            self.login.open = true;
        }
        if let Some(dir) = actions.open_dir {
            open_folder(&dir);
        }
        if actions.open_data_dir {
            self.save_ui_config();
            self.show_settings = false;
            self.open_data_dir();
        }
        if actions.refresh {
            self.refresh();
            self.notice = Some("已重新读取版本列表".into());
        }
        if actions.recheck_java {
            self.refresh_java();
        }
        if actions.check_update {
            self.check_update(ctx, true);
        }
        if actions.open_update {
            self.show_settings = false;
            self.update.open = true;
        }
        if actions.download_java {
            self.show_settings = false;
            self.start_java_download(21, None, None);
        }
        if actions.close || self.bench_confirm || actions.open_log {
            self.save_ui_config();
            self.show_settings = false;
        }
        if actions.open_log {
            self.open_log();
        }
    }

    /// 「游戏」：谁在玩、用哪个 Java、给多少内存、启动之后启动器怎么办。
    fn settings_game(&mut self, ui: &mut egui::Ui, actions: &mut SettingsActions) {
        settings_card(ui, |ui| {
            let current = self.accounts.current().cloned();
            let count = self.accounts.list.len();
            let face = self.face(ui.ctx(), current.as_ref().and_then(|a| a.skin.as_deref()));
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                let name = current.as_ref().map_or("?", |a| a.name.as_str());
                draw_avatar(ui.painter(), rect, name, face.as_ref());
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(name).size(14.0).family(bold_family()));
                    let (line, color) = match &current {
                        Some(a) if a.stale => {
                            (format!("{} · 需要重新登录", a.kind_label()), t::warn())
                        }
                        Some(a) if count > 1 => {
                            (format!("{} · 共 {count} 个账户", a.kind_label()), t::sub())
                        }
                        Some(a) => (a.kind_label(), t::sub()),
                        None => ("还没有账户".into(), t::sub()),
                    };
                    ui.label(egui::RichText::new(line).size(13.0).color(color));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if motion::secondary_sized(ui, "切换或添加", true, 112.0, 30.0, 10.5).clicked()
                    {
                        actions.open_account = true;
                    }
                });
            });
        });
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Java").size(13.0).color(t::sub()));
                ui.add_space(4.0);
                status_dot(ui, self.env.java_ok);
                let label = self.env.java_label.clone().unwrap_or_else(|| {
                    if self.env.java_ok {
                        "Java 21+ 已检测".to_string()
                    } else {
                        "未检测到 Java 21+".to_string()
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.env.java_ok
                        && action_link(ui, "下载 Java 21")
                            .on_hover_text("Mojang 官方的 Java 运行时，放在数据目录里，不改系统")
                            .clicked()
                    {
                        actions.download_java = true;
                    }
                    if action_link(ui, "重新检测").clicked() {
                        actions.recheck_java = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&label).size(13.0).color(
                                if self.env.java_ok {
                                    t::ink()
                                } else {
                                    t::warn()
                                },
                            ))
                            .truncate(),
                        )
                        .on_hover_text(match self.env.javas.len() {
                            0 => label.clone(),
                            n => format!(
                                "找到 {n} 个 Java：\n{}\n\n每个版本用哪个在「启动参数」里选",
                                self.env
                                    .javas
                                    .iter()
                                    .map(|java| format!(
                                        "{} · {}",
                                        java.label(),
                                        java.path.display()
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ),
                        });
                    });
                });
            });
        });
        ui.add_space(10.0);
        self.tuning_card(ui);
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            if toggle_row(
                ui,
                "游戏启动后收起启动器",
                "窗口出来后收到任务栏，游戏退出再回来",
                self.prefs.minimize_on_launch,
            ) {
                self.prefs.minimize_on_launch = !self.prefs.minimize_on_launch;
                self.save_ui_config();
            }
        });
    }

    /// 「启动器」：外观、缩放、动效、截图、更新和下载源。
    fn settings_launcher(&mut self, ui: &mut egui::Ui) {
        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("外观").size(13.0).color(t::sub()));
                ui.add_space(8.0);
                for pref in t::Preference::ALL {
                    if chip(ui, pref.label(), self.prefs.theme == pref).clicked() {
                        self.prefs.theme = pref;
                        pref.apply(ui.ctx());
                        self.save_ui_config();
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("缩放").size(13.0).color(t::sub()))
                    .on_hover_text("字和控件一起放大缩小。也可以按 Ctrl + 和 Ctrl −");
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                let current = crate::prefs::nearest_zoom(ui.ctx().zoom_factor());
                for zoom in crate::prefs::ZOOMS {
                    let label = format!("{:.0}%", zoom * 100.0);
                    if chip(ui, &label, (current - zoom).abs() < 0.01).clicked() {
                        ui.ctx().set_zoom_factor(zoom);
                        self.prefs.zoom = zoom;
                        self.save_ui_config();
                    }
                }
            });
            ui.add_space(6.0);
            if toggle_row(
                ui,
                "减少动效",
                "关闭过渡和持续动画",
                self.prefs.reduce_motion,
            ) {
                self.prefs.reduce_motion = !self.prefs.reduce_motion;
                motion::set_reduced(self.prefs.reduce_motion);
                self.save_ui_config();
            }
        });
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            if toggle_row(
                ui,
                "截图写入地点和人物",
                if self.prefs.embed_shots {
                    "写进图片的文字块，不改像素"
                } else {
                    "只保存在启动器里"
                },
                self.prefs.embed_shots,
            ) {
                self.prefs.embed_shots = !self.prefs.embed_shots;
                self.save_ui_config();
            }
            self.auto_update_row(ui);
        });
        ui.add_space(10.0);
        self.source_card(ui);
    }

    /// 「存储」：东西放在哪、能清掉多少。
    fn settings_storage(&mut self, ui: &mut egui::Ui, actions: &mut SettingsActions) {
        let game_dir = self
            .selected_row()
            .map(|row| row.game_dir.clone())
            .unwrap_or_else(|| self.root.dir().to_path_buf());
        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                if path_row(ui, "当前版本", &game_dir) {
                    actions.open_dir = Some(game_dir.clone());
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (open, change) = path_row_with(ui, "数据目录", self.root.dir(), Some("换"));
                if open {
                    actions.open_dir = Some(self.root.dir().to_path_buf());
                }
                actions.open_data_dir |= change;
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "版本、账户和设置都在数据目录里。可以换成别的启动器的 .minecraft",
                )
                .size(12.5)
                .color(t::dim()),
            );
        });
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            self.cleanup_row(ui);
        });
    }

    /// 「关于」：版本和更新、日志、快捷键。
    fn settings_about(&mut self, ui: &mut egui::Ui, actions: &mut SettingsActions) {
        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        egui::RichText::new(format!("Cairn {}", env!("CARGO_PKG_VERSION")))
                            .size(14.0)
                            .family(bold_family()),
                    );
                    ui.label(
                        egui::RichText::new("装得干净，坏了能救")
                            .size(13.0)
                            .color(t::sub()),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(version) = self.update.latest.as_ref().map(|r| r.version.clone()) {
                        if motion::secondary_sized(
                            ui,
                            &format!("更新到 {version}"),
                            true,
                            132.0,
                            30.0,
                            13.0,
                        )
                        .clicked()
                        {
                            actions.open_update = true;
                        }
                    } else if self.update.checking {
                        ui.label(egui::RichText::new("正在检查…").size(13.0).color(t::dim()));
                    } else {
                        if motion::secondary_sized(ui, "检查更新", true, 96.0, 30.0, 13.0).clicked()
                        {
                            actions.check_update = true;
                        }
                        if let Some(status) = &self.update.status {
                            ui.label(egui::RichText::new(status).size(13.0).color(t::sub()));
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if action_link(ui, "项目主页").clicked() {
                    open_url("https://github.com/dkjsiogu/cairn");
                }
                if action_link(ui, "更新记录").clicked() {
                    open_url("https://github.com/dkjsiogu/cairn/releases");
                }
                if action_link(ui, "反馈问题").clicked() {
                    open_url("https://github.com/dkjsiogu/cairn/issues");
                }
            });
        });
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(match &self.play.last {
                        Some(last) => format!("最近一次启动：{}", short_text(&last.label, 24)),
                        None => format!("启动器记录 {} 条", self.log.len()),
                    })
                    .size(13.0)
                    .color(t::sub()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if action_link(ui, "查看日志").clicked() {
                        actions.open_log = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} 个版本", self.rows.len()))
                        .size(13.0)
                        .color(t::sub()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if action_link(ui, "重新读取")
                        .on_hover_text("在文件管理器里动过 versions 文件夹之后用（F5）")
                        .clicked()
                    {
                        actions.refresh = true;
                    }
                });
            });
        });
        ui.add_space(10.0);
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("快捷键").size(13.0).color(t::sub()));
            ui.add_space(4.0);
            egui::Grid::new("settings_shortcuts")
                .num_columns(4)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    for (index, (keys, what)) in [
                        ("Ctrl+K", "找任何东西"),
                        ("Ctrl+1–5", "切换页面"),
                        ("Ctrl+F", "搜索"),
                        ("Ctrl+L", "日志"),
                        ("Ctrl+,", "设置"),
                        ("F5", "重新读取版本"),
                        ("Ctrl + / −", "缩放界面"),
                        ("Esc", "关闭弹窗"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        ui.label(egui::RichText::new(keys).size(13.0).monospace());
                        ui.label(egui::RichText::new(what).size(13.0).color(t::sub()));
                        if index % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });
    }
}

/// 设置弹窗里点了什么：画完以后统一处理。
#[derive(Default)]
struct SettingsActions {
    close: bool,
    open_account: bool,
    open_dir: Option<PathBuf>,
    open_data_dir: bool,
    refresh: bool,
    recheck_java: bool,
    download_java: bool,
    check_update: bool,
    open_update: bool,
    open_log: bool,
}

/// 设置的分栏。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum SettingsTab {
    #[default]
    Game,
    Launcher,
    Storage,
    About,
}

impl SettingsTab {
    pub(crate) const ALL: [SettingsTab; 4] =
        [Self::Game, Self::Launcher, Self::Storage, Self::About];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Game => "游戏",
            Self::Launcher => "启动器",
            Self::Storage => "存储",
            Self::About => "关于",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            Self::Game => "账户、Java、内存",
            Self::Launcher => "外观、缩放、更新、下载源",
            Self::Storage => "目录和清理",
            Self::About => "版本、日志、快捷键",
        }
    }
}

/// 设置里的一块：`bg` 底、细边，占满这一栏的宽度。
pub(crate) fn settings_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(t::bg())
        .stroke(egui::Stroke::new(1.0, t::hairline()))
        .corner_radius(motion::R)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// 一行开关：左边开关和名字，右边一句说明。整行都能点；返回是否点了。
pub(crate) fn toggle_row(ui: &mut egui::Ui, label: &str, hint: &str, on: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        clicked |= toggle_switch(ui, on).clicked();
        clicked |= ui
            .add(
                egui::Label::new(egui::RichText::new(label).size(13.0).color(if on {
                    t::ink()
                } else {
                    t::sub()
                }))
                .sense(egui::Sense::click()),
            )
            .clicked();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(hint).size(13.0).color(t::dim())).truncate(),
            );
        });
    });
    clicked
}

/// 两种下载源。镜像只是后备或加速，文件照样按官方给的校验值核对。
const SOURCES: [(&str, &str, &str); 2] = [
    ("official", "官方优先", "先连官方，连不上自动换镜像"),
    (
        "bmcl",
        "镜像优先",
        "游戏文件和加载器先走 BMCLAPI，国内通常更快；Mod 平台仍先连官方",
    ),
];

impl App {
    /// 下载源和镜像致谢。
    pub(crate) fn source_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(t::bg())
            .stroke(egui::Stroke::new(1.0, t::hairline()))
            .corner_radius(motion::R)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("下载源").size(13.0).color(t::sub()));
                    ui.add_space(8.0);
                    for (key, label, _) in SOURCES {
                        if chip(ui, label, self.prefs.source == key).clicked() {
                            self.prefs.source = key.to_string();
                            self.save_ui_config();
                        }
                    }
                });
                ui.add_space(4.0);
                let hint = SOURCES
                    .iter()
                    .find(|(key, _, _)| *key == self.prefs.source)
                    .map_or("", |(_, _, hint)| hint);
                ui.label(egui::RichText::new(hint).size(12.5).color(t::sub()));
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let dim = |text: &str| egui::RichText::new(text).size(12.5).color(t::dim());
                    let link = |text: &str| egui::RichText::new(text).size(12.5);
                    ui.label(dim("镜像由 "));
                    if ui
                        .link(link("BMCLAPI"))
                        .on_hover_text("bangbang93 维护的 Minecraft 下载镜像 · 点开看说明和赞助")
                        .clicked()
                    {
                        open_url("https://bmclapidoc.bangbang93.com/");
                    }
                    ui.label(dim(" 和 "));
                    if ui
                        .link(link("MCIM"))
                        .on_hover_text(
                            "mcmod-info-mirror 维护的 Modrinth / CurseForge 接口镜像 · 只在官方连不上时用",
                        )
                        .clicked()
                    {
                        open_url("https://github.com/mcmod-info-mirror");
                    }
                    ui.label(dim(" 免费提供，感谢维护者"));
                });
            });
    }
}

/// 内存：「自动」加 1 GB 步进。自动时显示算出来的值，按加减就转成手动。
pub(crate) fn memory_stepper(ui: &mut egui::Ui, memory: &mut Option<u32>, auto_mb: u32) {
    if chip(ui, "自动", memory.is_none()).clicked() {
        *memory = None;
    }
    ui.add_space(4.0);
    let (down, up) = match *memory {
        Some(mb) => {
            let gb = (mb as f32 / 1024.0).round() as u32;
            (gb.saturating_sub(1), gb + 1)
        }
        None => (auto_mb.div_ceil(1024).saturating_sub(1), auto_mb / 1024 + 1),
    };
    let button = |ui: &mut egui::Ui, text: &str, enabled: bool| {
        ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(text).size(13.0).color(t::sub()))
                .fill(t::bg())
                .stroke(egui::Stroke::new(1.0, t::hairline()))
                .corner_radius(motion::R)
                .min_size(egui::vec2(28.0, 28.0)),
        )
        .clicked()
    };
    if button(ui, "−", down >= 1) {
        *memory = Some(down * 1024);
    }
    let (text, color) = match *memory {
        Some(mb) => (tuning::gb(mb), t::ink()),
        None => (tuning::gb(auto_mb), t::sub()),
    };
    egui::Frame::new()
        .fill(t::surface())
        .stroke(egui::Stroke::new(1.0, t::hairline()))
        .corner_radius(motion::R)
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.add_sized(
                [52.0, 18.0],
                egui::Label::new(
                    egui::RichText::new(text)
                        .size(13.0)
                        .color(color)
                        .family(bold_family()),
                ),
            );
        });
    if button(ui, "+", up <= 16) {
        *memory = Some(up * 1024);
    }
}

impl App {
    /// 「自动更新」这一行。Windows 由启动器自己下载替换；Linux（有 apt 源）由启动器在后台
    /// 启动系统的更新服务。都可以关，关掉后只提示新版本。
    fn auto_update_row(&mut self, ui: &mut egui::Ui) {
        use nailong_core::update::Install;
        let kind = self.update.kind;
        if kind == Install::Other {
            return;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let on = match kind {
                Install::Exe | Install::AptRepo => {
                    if toggle_switch(ui, self.prefs.auto_update).clicked() {
                        self.prefs.auto_update = !self.prefs.auto_update;
                        self.save_ui_config();
                    }
                    self.prefs.auto_update
                }
                _ => false,
            };
            ui.label(egui::RichText::new("自动更新").size(13.0).color(if on {
                t::ink()
            } else {
                t::sub()
            }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = match kind {
                    Install::Exe if on => "打开时检查，关掉时换上",
                    Install::AptRepo if on => "打开时检查，在后台装好",
                    Install::Exe | Install::AptRepo => "只提示新版本",
                    _ => "没有 apt 源，需手动更新",
                };
                ui.label(egui::RichText::new(hint).size(13.0).color(t::sub()));
            });
        });
    }
}

enum CleanupMsg {
    Planned(Result<versions::Cleanup, String>),
    Done(Result<(usize, u64), String>),
}

/// 「清理不用的文件」：先算能空出多少，点了才删。见 docs/design/27-version-folders.md。
#[derive(Default)]
pub(crate) struct CleanupState {
    rx: Option<mpsc::Receiver<CleanupMsg>>,
    plan: Option<versions::Cleanup>,
    status: Option<String>,
}

impl App {
    fn cleanup_row(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.cleanup.rx {
            if let Ok(msg) = rx.try_recv() {
                self.cleanup.rx = None;
                match msg {
                    CleanupMsg::Planned(Ok(plan)) => {
                        self.cleanup.status = plan
                            .files
                            .is_empty()
                            .then(|| "没有可以清理的文件".to_string());
                        self.cleanup.plan = Some(plan).filter(|p| !p.files.is_empty());
                    }
                    CleanupMsg::Planned(Err(e)) | CleanupMsg::Done(Err(e)) => {
                        self.cleanup.status = Some(e);
                    }
                    CleanupMsg::Done(Ok((_, bytes))) => {
                        self.cleanup.status = Some(format!("已清理，空出 {}", human_size(bytes)));
                    }
                }
            }
            if self.cleanup.rx.is_some() {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        let working = self.cleanup.rx.is_some();
        // 游戏开着时也不清：它正在用这些文件。
        let blocked = self.busy.is_some() || self.any_game();
        let mut plan_now = false;
        let mut clean_now = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("不用的运行库和资源")
                    .size(13.0)
                    .color(t::sub()),
            )
            .on_hover_text("删掉版本以后，它用过的运行库、资源和安装缓存还留着");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if working {
                    ui.label(egui::RichText::new("正在处理…").size(13.0).color(t::dim()));
                    return;
                }
                let link = |ui: &mut egui::Ui, text: &str| {
                    let response = ui.add_enabled_ui(!blocked, |ui| action_link(ui, text));
                    if blocked {
                        response
                            .response
                            .on_disabled_hover_text("有任务或游戏在运行，完成后再清理");
                        false
                    } else {
                        response.inner.clicked()
                    }
                };
                match &self.cleanup.plan {
                    Some(plan) => {
                        if link(ui, "清理") {
                            clean_now = true;
                        }
                        ui.label(
                            egui::RichText::new(format!("能空出 {}", human_size(plan.bytes)))
                                .size(13.0),
                        );
                    }
                    None => {
                        if link(ui, "算一下") {
                            plan_now = true;
                        }
                        if let Some(status) = &self.cleanup.status {
                            ui.label(egui::RichText::new(status).size(13.0).color(t::sub()));
                        }
                    }
                }
            });
        });
        if plan_now {
            self.start_cleanup(ui.ctx(), false);
        }
        if clean_now {
            self.start_cleanup(ui.ctx(), true);
        }
    }

    /// 在后台算（或执行）清理。执行时重新算一遍，免得删掉刚装的版本要用的文件。
    fn start_cleanup(&mut self, ctx: &egui::Context, run: bool) {
        if self.busy.is_some() || self.any_game() || self.cleanup.rx.is_some() {
            return;
        }
        self.cleanup.plan = None;
        self.cleanup.status = None;
        let (tx, rx) = mpsc::channel();
        self.cleanup.rx = Some(rx);
        let root = self.root.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let plan = versions::plan_cleanup(&root).map_err(|e| e.to_string());
            let msg = match (plan, run) {
                (plan, false) => CleanupMsg::Planned(plan),
                (Err(e), true) => CleanupMsg::Done(Err(e)),
                (Ok(plan), true) => CleanupMsg::Done(
                    versions::run_cleanup(&root, &plan)
                        .map(|removed| (removed, plan.bytes))
                        .map_err(|e| e.to_string()),
                ),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }
}
