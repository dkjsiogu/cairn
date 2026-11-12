//! 启动台：当前版本、开始游戏、启动参数、摘要和截图墙。

use crate::prelude::*;

impl App {
    /// 启动台下方的截图墙：游戏真正写截图的目录（和启动时的游戏目录一致）。
    pub(crate) fn screenshot_strip(&mut self, ui: &mut egui::Ui, row: &Row, width: f32) {
        let game_dir = row.game_dir.clone();
        if self.gallery.shots.is_none() {
            self.gallery.shots = Some(shots::Shots::new(ui.ctx(), self.root.clone()));
            self.gallery.stale = true;
        }
        let Some(shots) = &mut self.gallery.shots else {
            return;
        };
        let (open_dir, open_gallery) = shots.strip(ui, &game_dir, width);
        if open_dir {
            let dir = game_dir.join("screenshots");
            let _ = std::fs::create_dir_all(&dir);
            open_folder(&dir);
        }
        if open_gallery {
            let version = shots
                .list
                .iter()
                .find(|shot| shot.game_dir == game_dir)
                .map(|shot| shot.version.clone())
                .unwrap_or_else(|| row.label.clone());
            self.open_gallery(Some(crate::pages::gallery::Filter {
                version: Some(version),
                ..Default::default()
            }));
        }
    }

    /// 首次启动：把 Java、目录和第一个版本排成三步，别一上来就甩表单。
    pub(crate) fn welcome_view(&mut self, ui: &mut egui::Ui) {
        let width = (ui.available_width() - 48.0).clamp(320.0, 720.0);
        let pad = ((ui.available_width() - width) / 2.0).max(0.0);
        let content_w = width - 48.0;
        let mut install = false;
        let mut import_pack = false;
        let mut recheck = false;
        let mut download_java = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(46.0);
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("欢迎使用 Cairn")
                                        .size(36.0)
                                        .family(light_family())
                                        .color(t::ink()),
                                );
                            });
                        });
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("装得干净，坏了能救。")
                                    .size(13.0)
                                    .color(t::ink()),
                            );
                            ui.label(
                                egui::RichText::new("第一次启动，只需要把这三件事准备好。")
                                    .size(13.0)
                                    .color(t::sub()),
                            );
                        });
                        ui.add_space(20.0);
                        egui::Frame::new()
                            .fill(t::surface())
                            .stroke(egui::Stroke::new(1.0, t::hairline()))
                            .corner_radius(motion::R)
                            .inner_margin(egui::Margin::symmetric(24, 20))
                            .show(ui, |ui| {
                                ui.set_width(content_w);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("开始之前")
                                            .size(15.0)
                                            .family(display_family()),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new("大约 1 分钟")
                                                    .size(13.0)
                                                    .color(t::sub()),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(12.0);

                                // 第一步：Java。没有 Java 不拦着下载，但先把路说清楚。
                                ui.horizontal_top(|ui| {
                                    onboarding_badge(ui, "1", self.env.java_ok);
                                    ui.add_space(10.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(content_w - 236.0, 1.0),
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.set_width(content_w - 236.0);
                                            ui.label(
                                                egui::RichText::new("确认 Java")
                                                    .size(15.0)
                                                    .family(bold_family()),
                                            );
                                            ui.label(
                                                egui::RichText::new(
                                                    "启动游戏需要 Java 21 或更高；下载版本本身不需要。",
                                                )
                                                .size(13.0)
                                                .color(t::sub()),
                                            );
                                            if let Some(label) = &self.env.java_label {
                                                ui.add_space(3.0);
                                                ui.label(
                                                    egui::RichText::new(short_text(label, 46))
                                                        .size(13.0)
                                                        .color(t::ok()),
                                                )
                                                .on_hover_text(label);
                                            }
                                        },
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.vertical(|ui| {
                                                if self.env.java_ok {
                                                    ui.horizontal(|ui| {
                                                        status_dot(ui, true);
                                                        ui.label(
                                                            egui::RichText::new("已就绪")
                                                                .size(13.0)
                                                                .color(t::ok()),
                                                        );
                                                    });
                                                } else {
                                                    ui.label(
                                                        egui::RichText::new("未找到合适的 Java")
                                                            .size(13.0)
                                                            .color(t::warn()),
                                                    );
                                                    ui.add_space(5.0);
                                                    if motion::secondary_sized(
                                                        ui,
                                                        "下载 Java 21",
                                                        true,
                                                        120.0,
                                                        30.0,
                                                        10.5,
                                                    )
                                                    .on_hover_text("下载 Mojang 官方的 Java 运行时，放在数据目录里，不改系统")
                                                    .clicked()
                                                    {
                                                        download_java = true;
                                                    }
                                                    if action_link(ui, "重新检测").clicked() {
                                                        recheck = true;
                                                    }
                                                }
                                            });
                                        },
                                    );
                                });
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(12.0);

                                // 第二步：目录。玩家只需要知道东西放在哪。
                                ui.horizontal_top(|ui| {
                                    onboarding_badge(ui, "2", true);
                                    ui.add_space(10.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(content_w - 236.0, 1.0),
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.set_width(content_w - 236.0);
                                            ui.label(
                                                egui::RichText::new("确认游戏目录")
                                                    .size(15.0)
                                                    .family(bold_family()),
                                            );
                                            ui.label(
                                                egui::RichText::new(
                                                    "版本、Mod 和存档都在这里，不写系统目录。",
                                                )
                                                .size(13.0)
                                                .color(t::sub()),
                                            );
                                        },
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.vertical(|ui| {
                                                let path = self.root.dir().display().to_string();
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(short_text(&path, 34))
                                                            .size(13.0)
                                                            .color(t::sub()),
                                                    )
                                                    .truncate(),
                                                )
                                                .on_hover_text(&path);
                                                if action_link(ui, "打开目录").clicked() {
                                                    open_folder(self.root.dir());
                                                }
                                            });
                                        },
                                    );
                                });
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(12.0);

                                // 第三步：只留一个主行动，整合包是次选。
                                ui.horizontal_top(|ui| {
                                    onboarding_badge(ui, "3", false);
                                    ui.add_space(10.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("装第一个游戏配置")
                                                .size(15.0)
                                                .family(bold_family()),
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                "原版 / Fabric / NeoForge 都可以；整合包会顺带装好加载器和 Mod。",
                                            )
                                            .size(13.0)
                                            .color(t::sub()),
                                        );
                                    });
                                });
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if motion::primary_sized_glyph(
                                        ui,
                                        "安装第一个版本",
                                        true,
                                        198.0,
                                        40.0,
                                        13.0,
                                        motion::PrimaryGlyph::Download,
                                    ) {
                                        install = true;
                                    }
                                    if motion::secondary_sized(
                                        ui,
                                        "导入整合包",
                                        true,
                                        112.0,
                                        38.0,
                                        11.0,
                                    )
                                    .clicked()
                                    {
                                        import_pack = true;
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(
                                                    "装完就能在启动台点开始",
                                                )
                                                .size(13.0)
                                                .color(t::sub()),
                                            );
                                        },
                                    );
                                });
                            });
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            status_dot(ui, true);
                            ui.label(
                                egui::RichText::new(
                                    "下载会校验完整性；内容变更前自动创建恢复点。Java 缺失时也能先装版本。",
                                )
                                .size(13.0)
                                .color(t::sub()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "v{}",
                                            env!("CARGO_PKG_VERSION")
                                        ))
                                        .size(13.0)
                                        .color(t::sub()),
                                    );
                                },
                            );
                        });
                        ui.add_space(24.0);
                    });
                });
            });
        if recheck {
            self.refresh_java();
        }
        if download_java {
            self.start_java_download(21, None, None);
        }
        if install {
            self.open_install();
        }
        if import_pack && let Some(path) = pick_modpack() {
            self.start_modpack(path);
        }
    }

    pub(crate) fn main_view(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<Row> = self.rows.clone();
        let Some(row) = self.selected_row().or_else(|| rows.first().cloned()) else {
            self.welcome_view(ui);
            return;
        };
        let game_dir = row.game_dir.clone();
        let mut cache = self.home_cache_for(&row, &game_dir);
        if self.verify_home_content(ui.ctx(), &row) {
            cache = self.home_cache_for(&row, &game_dir);
        }
        let ring_data = self.ring_data(&row, &cache);
        let running = ring_data.state == rings::State::Running;
        let session_status = self.session(&row.id).map(|session| session.status());
        let stat = self.play.stats.get(&row.id).cloned().unwrap_or_default();
        let (title, named) = home_title(&row);
        // 能不能启动后直接进最近的世界（1.20 起的版本）。
        let quick_world = cache
            .latest_world
            .as_ref()
            .filter(|_| !running && self.quick_play_ok(&row))
            .map(|world| world.name.clone());
        let compact_h = ui.ctx().content_rect().height() < 680.0;
        let hero_h: f32 = if compact_h { 230.0 } else { 290.0 };
        let width = (ui.available_width() - 72.0).clamp(320.0, 1000.0);
        let pad = ((ui.available_width() - width) / 2.0).max(0.0);

        let mut chosen: Option<String> = None;
        let mut primary = false;
        let mut clicked_layer = None;
        let mut open_saves = false;
        let mut open_content = false;
        let mut open_manage = false;
        let mut stop: Option<bool> = None;
        let mut enter_world = false;
        let stop_confirm = self.play.stop_confirm.as_deref() == Some(row.id.as_str());

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 内容不满一屏时整体略偏上居中，避免都挤在顶部。
                let spare = ui.available_height() - hero_h - 150.0;
                ui.add_space((spare * 0.4).max(if compact_h { 16.0 } else { 34.0 }));
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        let (hero, _) =
                            ui.allocate_exact_size(egui::vec2(width, hero_h), egui::Sense::hover());
                        let ring_w = hero_h * 300.0 / 260.0;
                        let ring_rect = egui::Rect::from_min_size(
                            egui::pos2(hero.right() - ring_w, hero.top()),
                            egui::vec2(ring_w, hero_h),
                        );
                        clicked_layer = rings::show(ui, ring_rect, &ring_data);

                        let left = egui::Rect::from_min_max(
                            hero.left_top(),
                            egui::pos2(ring_rect.left() - 12.0, hero.bottom()),
                        );
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(left)
                                .layout(egui::Layout::top_down(egui::Align::LEFT)),
                        );
                        child.add_space((hero_h - 206.0).max(0.0) / 2.0);
                        let status = match (&session_status, ring_data.state) {
                            (Some(status), _) => status.clone(),
                            (None, rings::State::Missing) => {
                                format!("缺 {} 个文件", row.health.missing)
                            }
                            // 玩过的写玩了多久；没玩过的只说装好了。
                            (None, _) if stat.seconds >= 60 => {
                                format!("玩了 {}", nailong_core::played::human(stat.seconds))
                            }
                            (None, _) => "安装完整".to_string(),
                        };
                        let loader = match &row.loader_version {
                            Some(version) => format!("{} {version}", row.loader.name()),
                            None => row.loader.name().to_string(),
                        };
                        // 起过名字的版本：大字是名字，这一行补上游戏版本。
                        let kind = if named {
                            format!("{} · {loader}", row.mc)
                        } else {
                            loader
                        };
                        child.label(
                            egui::RichText::new(format!("{kind} · {status}"))
                                .size(13.0)
                                .color(if row.health.missing > 0 {
                                    t::warn()
                                } else if running {
                                    t::accent()
                                } else {
                                    t::sub()
                                }),
                        );
                        // 版本名完整显示：放得下用大字，放不下按宽度缩小，太长才换成两行。
                        let (size, wrap) = fit_title(&child, &title, left.width());
                        let label = egui::Label::new(
                            egui::RichText::new(&title)
                                .size(size)
                                .family(light_family())
                                .color(t::ink()),
                        );
                        child
                            .add(if wrap { label.wrap() } else { label.extend() })
                            .on_hover_text(format!("{}\n{}", row.id, row_kind(&row)));
                        child.add_space(14.0);
                        child.horizontal(|ui| {
                            if running {
                                // 游戏开着：主按钮看这一局的输出，旁边能结束它。
                                if motion::secondary_sized(ui, "查看日志", true, 104.0, 40.0, 14.0)
                                    .on_hover_text("这一局的游戏输出，跟着游戏刷新")
                                    .clicked()
                                {
                                    primary = true;
                                }
                                if stop_confirm {
                                    if danger_button(ui, "确认结束", 96.0, 40.0)
                                        .on_hover_text("没存盘的进度会丢")
                                        .clicked()
                                    {
                                        stop = Some(true);
                                    }
                                    if motion::secondary_sized(ui, "取消", true, 64.0, 40.0, 14.0)
                                        .clicked()
                                    {
                                        stop = Some(false);
                                    }
                                } else if motion::secondary_sized(
                                    ui,
                                    "结束游戏",
                                    true,
                                    96.0,
                                    40.0,
                                    14.0,
                                )
                                .on_hover_text("游戏卡住时用。正常退出请在游戏里退")
                                .clicked()
                                {
                                    self.play.stop_confirm = Some(row.id.clone());
                                }
                            } else {
                                let (label, glyph) = match ring_data.state {
                                    rings::State::Missing => {
                                        ("补全文件", motion::PrimaryGlyph::Download)
                                    }
                                    _ => ("开始游戏", motion::PrimaryGlyph::Play),
                                };
                                if motion::primary_sized_glyph(
                                    ui, label, true, 150.0, 40.0, 14.0, glyph,
                                ) {
                                    primary = true;
                                }
                            }
                            if rows.len() > 1
                                && let Some(id) = self.version_switcher(ui, &rows, &row)
                            {
                                chosen = Some(id);
                            }
                        });
                        child.add_space(12.0);
                        self.launch_pills(&mut child, &row);
                        // 名字换成两行、窄窗口参数折行时，左边可能比预留的高：把多出来的让出来，
                        // 不压到下面的摘要。
                        let overflow = child.min_rect().bottom() - hero.bottom();
                        if overflow > 0.0 {
                            ui.add_space(overflow + 8.0);
                        }

                        ui.add_space(if compact_h { 10.0 } else { 18.0 });

                        // 三栏摘要：细线分隔，整栏可点。
                        let top = ui.cursor().top();
                        ui.painter().line_segment(
                            [egui::pos2(hero.left(), top), egui::pos2(hero.right(), top)],
                            egui::Stroke::new(1.0, t::hairline()),
                        );
                        let cell_w = width / 3.0;
                        let (cells, _) =
                            ui.allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover());
                        let world = match &cache.latest_world {
                            Some(world) => {
                                format!(
                                    "{} · {}",
                                    short_text(&world.name, 14),
                                    human_age(world.modified)
                                )
                            }
                            None => "还没有世界".to_string(),
                        };
                        let content = if ring_data.update_names.is_empty() {
                            format!("{} 个 Mod · {} 个资源包", cache.mods, cache.packs)
                        } else {
                            format!("{} 个 Mod 可更新", ring_data.update_names.len())
                        };
                        let backup = match &ring_data.latest_snapshot {
                            Some(age) => format!("{} 个快照 · 最近 {age}", cache.snapshots),
                            None => "还没有快照".to_string(),
                        };
                        for (index, (name, detail)) in
                            [("最近世界", world), ("内容", content), ("备份", backup)]
                                .into_iter()
                                .enumerate()
                        {
                            let rect = egui::Rect::from_min_size(
                                cells.left_top() + egui::vec2(cell_w * index as f32, 0.0),
                                egui::vec2(cell_w, cells.height()),
                            );
                            // 「最近世界」右边一个小按钮：启动后直接进这个世界。先占住它的位置，
                            // 再画整栏，这样点按钮不会同时算成点了这一栏。
                            let enter = (index == 0).then_some(quick_world.as_ref()).flatten().map(
                                |world| {
                                    let button = egui::Rect::from_center_size(
                                        egui::pos2(rect.right() - 52.0, rect.center().y),
                                        egui::vec2(72.0, 30.0),
                                    );
                                    (button, world)
                                },
                            );
                            let cell = summary_cell(ui, rect, name, &detail, index > 0);
                            let mut on_button = false;
                            if let Some((button, world)) = enter {
                                // 用子区域画，不动外面的排版位置。
                                let mut child =
                                    ui.new_child(egui::UiBuilder::new().max_rect(button));
                                let response = motion::secondary_sized(
                                    &mut child, "进入", true, 72.0, 30.0, 13.0,
                                )
                                .on_hover_text(format!("开始游戏，直接进「{world}」"));
                                on_button = response.hovered();
                                if response.clicked() {
                                    enter_world = true;
                                }
                            }
                            if cell.clicked() && !on_button {
                                match index {
                                    1 => open_content = true,
                                    _ => open_saves = true,
                                }
                            }
                        }

                        self.screenshot_strip(ui, &row, width);
                        ui.add_space(16.0);
                    });
                });
            });

        if let Some(id) = chosen {
            self.selected = Some(id);
            self.play.stop_confirm = None;
        }
        match stop {
            Some(true) => self.stop_game(&row.id),
            Some(false) => self.play.stop_confirm = None,
            None => {}
        }
        if enter_world && let Some(world) = quick_world {
            self.launch_row(&row, Some(launch::QuickPlay::World(world)));
        }
        match clicked_layer {
            Some(rings::Layer::Core) => primary = true,
            Some(rings::Layer::Files) => open_manage = true,
            Some(rings::Layer::Mods) => open_content = true,
            Some(rings::Layer::Saves) => open_saves = true,
            None => {}
        }
        if primary {
            if running {
                self.open_game_log(&row.id);
            } else if row.health.missing > 0 {
                self.start_repair(row.clone());
            } else {
                self.start_launch();
            }
        }
        if open_manage {
            self.nav_go(Nav::Versions);
        }
        if open_saves {
            self.open_saves(row.clone());
        }
        if open_content {
            self.open_content(true);
        }
    }

    /// 这个版本能不能启动后直接进世界。要读版本文件，结果按版本记着，列表刷新时清掉。
    pub(crate) fn quick_play_ok(&mut self, row: &Row) -> bool {
        if let Some(known) = self.home.quick_play.get(&row.id) {
            return *known;
        }
        let ok = launch::can_quick_play_world(&self.root, &row.profile);
        self.home.quick_play.insert(row.id.clone(), ok);
        ok
    }

    /// 启动台的「切换版本」：列出版本名（最近玩过的在前），带游戏版本和加载器；
    /// 正在运行的前面有圆点。版本多的时候能搜。返回选中的版本。
    pub(crate) fn version_switcher(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[Row],
        current: &Row,
    ) -> Option<String> {
        let mut chosen = None;
        let running: HashSet<&str> = self
            .play
            .sessions
            .iter()
            .map(|session| session.row_id.as_str())
            .collect();
        ui.spacing_mut().interact_size.y = 40.0;
        let searchable = rows.len() > 8;
        let query = self.home.switch_query.trim().to_lowercase();
        let response = egui::ComboBox::from_id_salt("home_version_switch")
            .selected_text(egui::RichText::new("切换版本").size(14.0))
            .width(96.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.set_min_width(280.0);
                if searchable {
                    let field = search_field(ui, &mut self.home.switch_query, "搜索版本", 252.0);
                    if !field.has_focus() && self.home.switch_query.is_empty() {
                        field.request_focus();
                    }
                    ui.add_space(4.0);
                }
                let mut shown = 0;
                for item in rows {
                    let kind = row_kind(item);
                    if !query.is_empty()
                        && !item.id.to_lowercase().contains(&query)
                        && !kind.to_lowercase().contains(&query)
                    {
                        continue;
                    }
                    shown += 1;
                    if switch_row(
                        ui,
                        &item.id,
                        &kind,
                        item.id == current.id,
                        running.contains(item.id.as_str()),
                    )
                    .clicked()
                    {
                        chosen = Some(item.id.clone());
                    }
                }
                if shown == 0 {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("没有符合的版本")
                            .size(13.0)
                            .color(t::dim()),
                    );
                    ui.add_space(6.0);
                }
            });
        if response.inner.is_none() {
            // 列表关着：下次打开从头搜。
            self.home.switch_query.clear();
        }
        if chosen.is_some() {
            self.home.switch_query.clear();
            ui.close();
        }
        chosen.filter(|id| id != &current.id)
    }

    pub(crate) fn home_cache_for(&mut self, row: &Row, game_dir: &std::path::Path) -> HomeCache {
        if let Some(cache) = self.home.cache.as_ref()
            && cache.row_id == row.id
        {
            return cache.clone();
        }
        let dir = row.game_dir.clone();
        let index = installed::load(&dir);
        let count = |kind: mods::ContentKind| {
            index
                .entries
                .iter()
                .filter(|entry| entry.kind == kind)
                .count()
        };
        let mut worlds = saves::worlds(game_dir).unwrap_or_default();
        worlds.sort_by_key(|world| world.modified);
        let mut snapshot_count = 0;
        let mut latest_snapshot: Option<u64> = None;
        for world in &worlds {
            let (count, latest) = snapshots::summary(game_dir, &world.name).unwrap_or((0, None));
            snapshot_count += count;
            latest_snapshot = latest_snapshot.max(latest);
        }
        let latest_world = worlds.pop();
        let mod_entries = index
            .entries
            .iter()
            .filter(|entry| entry.kind == mods::ContentKind::Mod)
            .map(|entry| (entry.file.clone(), entry.name.clone(), entry.enabled))
            .collect();
        let cache = HomeCache {
            row_id: row.id.clone(),
            mods: count(mods::ContentKind::Mod),
            packs: count(mods::ContentKind::ResourcePack),
            latest_world,
            mod_entries,
            snapshots: snapshot_count,
            latest_snapshot,
        };
        self.home.cache = Some(cache.clone());
        cache
    }

    /// 启动台先用索引里的数字，后台核对一遍内容目录（只看大小和修改时间，没变的不打开），
    /// 结果回来就更新。返回 true 表示刚更新过缓存。
    pub(crate) fn verify_home_content(&mut self, ctx: &egui::Context, row: &Row) -> bool {
        if let Some((id, rx)) = &self.home.scan {
            match rx.try_recv() {
                Ok(entries) => {
                    let id = id.clone();
                    self.home.scan = None;
                    if let Some(cache) = &mut self.home.cache
                        && cache.row_id == id
                    {
                        let count = |kind: mods::ContentKind| {
                            entries.iter().filter(|entry| entry.kind == kind).count()
                        };
                        cache.mods = count(mods::ContentKind::Mod);
                        cache.packs = count(mods::ContentKind::ResourcePack);
                        cache.mod_entries = entries
                            .iter()
                            .filter(|entry| entry.kind == mods::ContentKind::Mod)
                            .map(|entry| (entry.file.clone(), entry.name.clone(), entry.enabled))
                            .collect();
                        self.home.verified = Some(id);
                        return true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => self.home.scan = None,
            }
        }
        let fresh = self
            .home
            .cache
            .as_ref()
            .is_some_and(|cache| cache.row_id == row.id)
            && self.home.verified.as_deref() == Some(row.id.as_str());
        if fresh || self.busy.is_some() {
            return false;
        }
        let dir = row.game_dir.clone();
        let (tx, rx) = mpsc::channel();
        self.home.scan = Some((row.id.clone(), rx));
        self.home.verified = Some(row.id.clone());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let entries = if dir.is_dir() {
                installed::scan_all(&dir)
                    .map(|index| index.entries)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let _ = tx.send(entries);
            ctx.request_repaint();
        });
        false
    }

    /// 把缓存和当前任务整理成状态圆环要显示的数据。
    pub(crate) fn ring_data(&self, row: &Row, cache: &HomeCache) -> rings::Data {
        let updates: Vec<&installed::UpdateCandidate> = if self
            .content
            .row
            .as_ref()
            .is_some_and(|target| target.id == row.id)
        {
            self.content.updates.iter().collect()
        } else {
            Vec::new()
        };
        let mods = cache
            .mod_entries
            .iter()
            .map(|(file, _, enabled)| rings::ModDot {
                enabled: *enabled,
                update: updates.iter().any(|update| update.entry.file == *file),
            })
            .collect();
        let update_names = updates
            .iter()
            .map(|update| update.entry.name.clone())
            .collect();
        let missing = row.health.missing;
        let session = self.session(&row.id);
        let state = if session.is_some() {
            rings::State::Running
        } else if missing > 0 {
            rings::State::Missing
        } else {
            rings::State::Ready
        };
        rings::Data {
            state,
            // 健康检查没有总文件数，这里只表达“缺得越多缺口越大”。
            fraction: 1.0 - missing as f32 / (missing as f32 + 60.0),
            missing: missing as usize,
            progress_detail: String::new(),
            mods,
            update_names,
            snapshots: cache.snapshots,
            latest_snapshot: cache.latest_snapshot.map(human_age),
            running_minutes: session
                .map(|session| session.started.elapsed().as_secs() / 60)
                .unwrap_or(0),
        }
    }
}

/// 启动台的大标题，以及它是不是玩家起的名字。默认名字（`1.21.1`、`1.21.1 NeoForge`）和别的
/// 启动器那种 `neoforge-21.1.209` 只显示游戏版本，加载器写在上面一行；整合包显示整合包的名字；
/// 玩家自己起的名字原样显示。
pub(crate) fn home_title(row: &Row) -> (String, bool) {
    if let Some(pack) = &row.pack {
        return (pack.clone(), true);
    }
    let id = row.id.as_str();
    let lower = id.to_lowercase();
    let loader_word = match row.loader {
        LoaderKind::Vanilla => "",
        LoaderKind::Fabric => "fabric",
        LoaderKind::NeoForge => "neoforge",
        LoaderKind::Forge => "forge",
    };
    // `fabric-loader-0.16.10-1.21.1`、`1.21.1-forge-52.1.0`：去掉加载器的名字只剩版本号。
    let only_versions = |text: &str| {
        text.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '_' | ' '))
    };
    let default_like = id == row.mc
        || id == versions::default_name(&row.mc, row.loader.core())
        || (!loader_word.is_empty()
            && lower.contains(loader_word)
            && only_versions(&lower.replace(loader_word, "").replace("loader", "")));
    if default_like || row.mc.is_empty() {
        let title = if row.mc.is_empty() { id } else { &row.mc };
        (title.to_string(), false)
    } else {
        (id.to_string(), true)
    }
}

/// 启动台大标题的字号：64 号放得下就用 64，放不下按宽度缩小，最小 30；
/// 30 号还放不下就换行（返回 true）。
pub(crate) fn fit_title(ui: &egui::Ui, text: &str, max_width: f32) -> (f32, bool) {
    const MAX: f32 = 64.0;
    const MIN: f32 = 30.0;
    let measured = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            text.to_string(),
            egui::FontId::new(MAX, light_family()),
            egui::Color32::WHITE,
        )
        .size()
        .x
    });
    if measured <= max_width {
        return (MAX, false);
    }
    let size = (MAX * max_width / measured * 0.98).floor();
    if size >= MIN {
        (size, false)
    } else {
        // 两行能放下的最大字号，留一点余量给断行位置。
        (
            (MAX * 2.0 * max_width / measured * 0.9)
                .floor()
                .clamp(24.0, MIN),
            true,
        )
    }
}

/// 「切换版本」里的一行：版本名一行，游戏版本和加载器一行；正在运行的前面一个圆点。
fn switch_row(
    ui: &mut egui::Ui,
    name: &str,
    kind: &str,
    active: bool,
    running: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    list_row_fill(ui, rect, active, &response);
    let left = rect.left() + 12.0;
    let text_w = rect.width() - 24.0 - if running { 14.0 } else { 0.0 };
    let name_font = egui::FontId::proportional(14.0);
    let kind_font = egui::FontId::proportional(12.0);
    ui.painter().text(
        egui::pos2(left, rect.top() + 14.0),
        egui::Align2::LEFT_CENTER,
        elide_middle(ui, name, &name_font, text_w),
        name_font,
        t::ink(),
    );
    ui.painter().text(
        egui::pos2(left, rect.top() + 31.0),
        egui::Align2::LEFT_CENTER,
        elide_middle(ui, kind, &kind_font, text_w),
        kind_font,
        t::dim(),
    );
    if running {
        ui.painter().circle_filled(
            egui::pos2(rect.right() - 14.0, rect.center().y),
            4.0,
            t::accent(),
        );
    }
    if running {
        response.on_hover_text("正在运行")
    } else {
        response
    }
}

/// 启动台的一栏摘要。
pub(crate) fn summary_cell(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    title: &str,
    detail: &str,
    divider: bool,
) -> egui::Response {
    let response = ui.interact(
        rect,
        egui::Id::unique("summary_cell").with(title),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("hover"), response.hovered());
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect.shrink(4.0), motion::R, t::tint(t::wash(), hover));
    }
    if divider {
        ui.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            egui::Stroke::new(1.0, t::hairline()),
        );
    }
    let left = rect.left() + if divider { 18.0 } else { 8.0 };
    ui.painter().text(
        egui::pos2(left, rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::new(15.0, bold_family()),
        t::ink(),
    );
    ui.painter().text(
        egui::pos2(left, rect.top() + 44.0),
        egui::Align2::LEFT_TOP,
        detail,
        egui::FontId::proportional(13.0),
        t::sub(),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 启动台：摘要缓存、后台核对和截图墙。
pub(crate) struct HomeState {
    pub(crate) cache: Option<HomeCache>,
    /// 启动台在后台核对内容目录：(版本行 id, 结果)。索引可能是旧的或从没扫过。
    pub(crate) scan: Option<(String, mpsc::Receiver<Vec<installed::InstalledEntry>>)>,
    /// 这一轮缓存里已经核对过的版本行。
    pub(crate) verified: Option<String>,
    /// 每个版本能不能启动后直接进世界（读版本文件得出，列表刷新时清掉）。
    pub(crate) quick_play: HashMap<String, bool>,
    /// 「切换版本」里的搜索词。
    pub(crate) switch_query: String,
}
