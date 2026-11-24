//! 页面导航和侧边栏。五个一级页面：启动台、版本、内容、存档、截图。

use crate::prelude::*;

pub(crate) mod content;
pub(crate) mod gallery;
pub(crate) mod home;
pub(crate) mod progress;
pub(crate) mod shots;
pub(crate) mod versions;
pub(crate) mod worlds;

impl App {
    /// 当前侧边栏应该高亮哪一项。整页显示进度时跟着任务走，其余跟着页面走。
    pub(crate) fn nav_active(&self) -> Nav {
        if self.progress_showing() {
            match self.busy {
                Some(Task::Install) => return Nav::Versions,
                Some(Task::Mods) => return Nav::Content,
                _ => {}
            }
        }
        self.page.nav()
    }

    pub(crate) fn nav_go(&mut self, nav: Nav) {
        if self.page_locked() {
            return;
        }
        // 下载、安装时点侧栏：离开进度页，任务在后台继续。
        let was_progress = self.progress_showing();
        self.progress_open = false;
        if self.page.nav() == nav && !was_progress {
            // 再点一次当前入口：只做“回到这个入口的默认视图”，不重置数据。
            match nav {
                Nav::Versions if self.page == Page::Install && !self.rows.is_empty() => {
                    self.page = Page::Versions;
                    self.install.repair_target = None;
                }
                Nav::Content if !self.content.show_installed => self.open_content(true),
                // 再点一次「截图」回到总览。
                Nav::Shots => {
                    self.gallery.tab = gallery::GalleryTab::Overview;
                    self.gallery.filter = gallery::Filter::default();
                    self.gallery.query.clear();
                }
                _ => {}
            }
            return;
        }
        match nav {
            Nav::Home => {
                self.page = Page::Home;
                self.install.repair_target = None;
                // 首页只是切视图：版本列表已在安装/删除/内容变更后刷新过。
                // 不再在这里同步 scan 每个版本的 health，否则点击会卡一下。
                self.ensure_visible_selection();
            }
            Nav::Versions => self.open_versions(),
            Nav::Content => self.open_content(true),
            Nav::Saves => self.open_saves_page(),
            Nav::Shots => self.open_gallery(None),
        }
    }

    /// 进版本页；一个版本都没有时直接去安装。
    pub(crate) fn open_versions(&mut self) {
        if self.page_locked() {
            return;
        }
        if self.rows.is_empty() {
            self.open_install();
            return;
        }
        self.go(Page::Versions);
        self.install.repair_target = None;
        self.delete_confirm = None;
        self.ensure_visible_selection();
    }

    /// 内容只有一页：发现和已安装是同一页的两种模式。
    pub(crate) fn open_content(&mut self, installed: bool) {
        if self.page_locked() {
            return;
        }
        let Some(row) = self.selected_row().or_else(|| self.rows.first().cloned()) else {
            self.open_install();
            return;
        };
        self.selected = Some(row.id.clone());
        let was_content = self.page == Page::Content;
        self.go(Page::Content);
        let same_target = self
            .content
            .row
            .as_ref()
            .is_some_and(|current| current.id == row.id);
        if !was_content || !same_target {
            self.enter_mods(Some(row));
        }
        self.clear_mods_selection();
        self.content.show_installed = installed;
        self.content.filter.clear();
        self.content.delete_confirm = None;
        self.content.error = None;
        self.content.notice = None;
        if installed {
            if self.content.game_dir.is_some() {
                self.start_mods_scan();
            }
        } else {
            self.content.searching = false;
            self.content.shelf = None;
            self.content.query.clear();
            if self.content.browse.is_empty() && !self.content.browse_loading {
                self.start_browse();
            }
        }
    }

    pub(crate) fn open_saves_page(&mut self) {
        if self.page_locked() {
            return;
        }
        let Some(row) = self.selected_row().or_else(|| self.rows.first().cloned()) else {
            self.open_install();
            return;
        };
        self.selected = Some(row.id.clone());
        self.open_saves(row);
    }

    pub(crate) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let wide = ui.ctx().content_rect().width() >= 1080.0;
        let width = if wide { 204.0 } else { 66.0 };
        let active = self.nav_active();
        let current = self.current_account().cloned();
        let face = self.face(ui.ctx(), current.as_ref().and_then(|a| a.skin.as_deref()));
        let mut account_rect = None;
        let login_busy = self.login.busy;
        let enabled = !self.page_locked();
        let mut go: Option<Nav> = None;
        let mut account_clicked = false;
        let mut update_clicked = false;
        let update_badge = self.update.badge(&self.prefs.update_skip);
        let mut settings_clicked = false;
        let mut open_progress = false;
        let mut open_palette = false;
        let mut open_game: Option<String> = None;

        egui::Panel::left("shell_nav")
            .resizable(false)
            .exact_size(width)
            .frame(
                egui::Frame::new()
                    .fill(t::rail())
                    .inner_margin(egui::Margin {
                        left: if wide { 12 } else { 10 },
                        right: if wide { 12 } else { 10 },
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                if wide {
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        cairn_mark(ui, 28.0);
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Cairn")
                                .size(19.0)
                                .family(bold_family())
                                .color(t::ink()),
                        );
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        cairn_mark(ui, 30.0);
                    });
                }
                ui.add_space(12.0);
                if sidebar_search(ui, wide, enabled).clicked() {
                    open_palette = true;
                }
                ui.add_space(10.0);

                let nav_bg_slot = ui.painter().add(egui::Shape::Noop);
                let nav_bar_slot = ui.painter().add(egui::Shape::Noop);
                let mut active_nav_rect = None;
                for nav in Nav::ALL {
                    let resp = sidebar_item(ui, nav, active == nav, wide, enabled);
                    if active == nav {
                        active_nav_rect = Some(resp.rect);
                    }
                    if resp.clicked() {
                        go = Some(nav);
                    }
                }
                if let Some(target) = active_nav_rect {
                    let pill = motion::spring_rect(
                        ui.ctx(),
                        egui::Id::unique("cairn_sidebar_pill"),
                        target,
                    );
                    ui.painter().set(
                        nav_bg_slot,
                        backdrop::nav_active_shape(ui.ctx(), pill, motion::R + 1),
                    );
                    ui.painter()
                        .set(nav_bar_slot, active_accent_bar_shape(pill));
                }

                // 底部用显式留白定位：嵌套 bottom_up 在侧边栏里会缩成内容高度。
                let games: Vec<(String, String)> = self
                    .play
                    .sessions
                    .iter()
                    .map(|session| {
                        let status = match &session.target {
                            Some(target) => format!("{} · {target}", session.status()),
                            None => session.status(),
                        };
                        (session.row_id.clone(), status)
                    })
                    .collect();
                // 下载、安装在后台进行：底部一张进度卡，点它回到进度页。
                let task_card = self
                    .busy
                    .filter(|task| task.shows_progress() && *task != Task::Bench)
                    .filter(|_| !self.progress_open);
                let locked = login_busy || matches!(self.busy, Some(Task::Bench));
                let footer =
                    16.0 + if update_badge.is_some() && !locked {
                        38.0
                    } else {
                        0.0
                    } + if locked {
                        40.0
                    } else {
                        88.0 + if task_card.is_some() {
                            if wide { 64.0 } else { 44.0 }
                        } else {
                            0.0
                        }
                    } + if locked {
                        0.0
                    } else {
                        games.len() as f32 * 44.0 + if games.is_empty() { 0.0 } else { 6.0 }
                    };
                ui.add_space((ui.available_height() - footer).max(10.0));
                let div_y = ui.cursor().top();
                ui.painter().line_segment(
                    [
                        egui::pos2(ui.min_rect().left() + 4.0, div_y),
                        egui::pos2(ui.min_rect().right() - 4.0, div_y),
                    ],
                    egui::Stroke::new(1.0, t::hairline()),
                );
                ui.add_space(10.0);
                if !locked {
                    // 正在运行的游戏：一个一行，点它看这一局的输出。
                    for (row_id, status) in &games {
                        if game_row(ui, row_id, status, wide).clicked() {
                            open_game = Some(row_id.clone());
                        }
                    }
                    if !games.is_empty() {
                        ui.add_space(6.0);
                    }
                }
                if locked {
                    ui.horizontal(|ui| {
                        if wide {
                            ui.add_space(12.0);
                        }
                        busy_spinner(ui);
                        if wide {
                            ui.label(
                                egui::RichText::new(if login_busy {
                                    "正在登录…"
                                } else {
                                    "正在测试"
                                })
                                .size(13.0)
                                .color(t::sub()),
                            );
                        }
                    });
                } else {
                    if let Some(task) = task_card {
                        let verb = match task {
                            Task::Mods => "正在下载内容",
                            Task::Export => "正在导出",
                            Task::Java => "正在下载 Java",
                            _ => "正在安装",
                        };
                        let (fraction, detail, hover) = match &self.activity {
                            Some(activity) => (
                                activity.overall(),
                                activity
                                    .eta()
                                    .map(remaining)
                                    .or_else(|| activity.active().last().map(|s| s.to_string()))
                                    .unwrap_or_default(),
                                activity.title.clone(),
                            ),
                            None => (None, String::new(), String::new()),
                        };
                        let response = task_card_widget(ui, verb, fraction, &detail, wide);
                        let response = if hover.is_empty() {
                            response
                        } else {
                            response.on_hover_text(format!("{verb} · {hover}\n点开看详细进度"))
                        };
                        if response.clicked() {
                            open_progress = true;
                        }
                        ui.add_space(8.0);
                    }
                    if let Some((version, ready)) = &update_badge {
                        if update_pill(ui, version, *ready, wide) {
                            update_clicked = true;
                        }
                        ui.add_space(8.0);
                    }
                    let response = sidebar_account(ui, current.as_ref(), face.as_ref(), wide);
                    account_rect = Some(response.rect);
                    if response.clicked() {
                        account_clicked = true;
                    }
                    if sidebar_settings(ui, wide, true).clicked() {
                        settings_clicked = true;
                    }
                }
            });

        if let Some(nav) = go {
            self.nav_go(nav);
        }
        if open_palette && !self.page_locked() {
            self.open_palette();
        }
        if open_progress {
            self.progress_open = true;
        }
        if let Some(row_id) = open_game {
            self.open_game_log(&row_id);
        }
        if account_rect.is_some() {
            self.login.anchor = account_rect;
        }
        if account_clicked {
            self.login.open = !self.login.open;
        }
        if update_clicked {
            self.update.open = true;
        }
        if settings_clicked {
            self.open_settings();
        }
    }
}

/// 宝石蓝渐变压印方徽章内的三石路标：Cairn 的品牌徽记。
fn cairn_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let r = (size * 0.28).clamp(6.0, 9.0) as u8;
    ui.painter()
        .add(t::glow_shadow(t::accent(), 0.45).as_shape(rect, r));
    backdrop::paint_primary_fill(ui.ctx(), ui.painter(), rect, r, 0.0, false);
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(64)),
        egui::StrokeKind::Inside,
    );
    let cx = rect.center().x;
    let bottom = rect.bottom() - size * 0.20;
    let s = size / 28.0;
    let painter = ui.painter();
    let stone_a = egui::Color32::from_white_alpha(175);
    let stone_b = egui::Color32::from_white_alpha(215);
    let stone_c = egui::Color32::WHITE;
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(cx, bottom - 2.2 * s),
            egui::vec2(16.0 * s, 3.8 * s),
        ),
        2.0 * s,
        stone_a,
    );
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(cx, bottom - 7.4 * s),
            egui::vec2(11.4 * s, 3.6 * s),
        ),
        1.8 * s,
        stone_b,
    );
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(cx, bottom - 12.2 * s),
            egui::vec2(6.8 * s, 3.4 * s),
        ),
        1.7 * s,
        stone_c,
    );
}

/// 侧栏顶部的全局搜索入口（打开 `Ctrl+K` 命令面板）。
fn sidebar_search(ui: &mut egui::Ui, wide: bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), sense);
    let hover = motion::hover_amount(
        ui,
        response.id.with("ssearch"),
        enabled && response.hovered(),
    );
    ui.painter().rect(
        rect,
        motion::R,
        motion::mix(t::surface(), t::elevated(), hover),
        egui::Stroke::new(1.0, motion::mix(t::hairline(), t::accent(), 0.65 * hover)),
        egui::StrokeKind::Inside,
    );
    let icon_color = motion::mix(t::dim(), t::accent(), hover);
    let c = if wide {
        egui::pos2(rect.left() + 14.0, rect.center().y - 1.0)
    } else {
        egui::pos2(rect.center().x - 1.0, rect.center().y - 1.0)
    };
    ui.painter()
        .circle_stroke(c, 4.2, egui::Stroke::new(1.4, icon_color));
    ui.painter().line_segment(
        [c + egui::vec2(3.1, 3.1), c + egui::vec2(6.0, 6.0)],
        egui::Stroke::new(1.5, icon_color),
    );
    if wide {
        ui.painter().text(
            egui::pos2(rect.left() + 27.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "快速搜索",
            egui::FontId::proportional(12.5),
            motion::mix(t::dim(), t::ink(), hover),
        );
        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 25.0, rect.center().y),
            egui::vec2(38.0, 19.0),
        );
        ui.painter().rect(
            badge,
            5,
            t::rail(),
            egui::Stroke::new(1.0, t::hairline()),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            "Ctrl+K",
            egui::FontId::proportional(10.5),
            t::sub(),
        );
    }
    response.on_hover_text("快速搜索版本、世界、页面与操作（Ctrl+K）")
}

/// 侧栏里一个正在运行的游戏：翠绿实时状态卡，点它看这一局的输出。
fn game_row(ui: &mut egui::Ui, name: &str, status: &str, wide: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    let hover = motion::hover_amount(ui, response.id.with("game_hover"), response.hovered());
    ui.painter().rect(
        rect,
        motion::R,
        t::tint(t::ok(), 0.10 + 0.06 * hover),
        egui::Stroke::new(1.0, t::tint(t::ok(), 0.30 + 0.25 * hover)),
        egui::StrokeKind::Inside,
    );
    let center = if wide {
        egui::pos2(rect.left() + 18.0, rect.center().y)
    } else {
        rect.center()
    };
    ui.painter()
        .circle_filled(center, 7.0, t::tint(t::ok(), 0.24));
    ui.painter().circle_filled(center, 3.6, t::ok());
    if wide {
        let font = egui::FontId::new(12.5, bold_family());
        let width = rect.right() - 10.0 - (rect.left() + 34.0);
        ui.painter().text(
            egui::pos2(rect.left() + 34.0, rect.center().y - 7.5),
            egui::Align2::LEFT_CENTER,
            elide_middle(ui, name, &font, width),
            font,
            t::ink(),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 34.0, rect.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            status,
            egui::FontId::proportional(11.5),
            t::ok(),
        );
    }
    response.on_hover_text(format!("{name} · {status}\n点开看游戏输出"))
}

/// 侧栏底部的进度卡：下载、安装在后台进行时显示。宽侧栏两行（在做什么 · 百分比，剩余时间），
/// 下面一条细进度条；窄侧栏只有百分比和进度条。
fn task_card_widget(
    ui: &mut egui::Ui,
    verb: &str,
    fraction: Option<f32>,
    detail: &str,
    wide: bool,
) -> egui::Response {
    let height = if wide { 54.0 } else { 36.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let hover = motion::hover_amount(ui, response.id.with("hover"), response.hovered());
    ui.painter().rect_filled(
        rect,
        motion::R,
        motion::mix(t::tint(t::wash(), 0.6), t::wash(), hover),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let inner = rect.shrink2(egui::vec2(if wide { 12.0 } else { 6.0 }, 8.0));
    let percent = fraction.map(|f| format!("{:.0}%", f * 100.0));
    let painter = ui.painter();
    if wide {
        painter.text(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            verb,
            egui::FontId::proportional(13.0),
            t::ink(),
        );
        if let Some(percent) = &percent {
            painter.text(
                inner.right_top(),
                egui::Align2::RIGHT_TOP,
                percent,
                egui::FontId::proportional(13.0),
                t::ink(),
            );
        }
        let detail = elide_middle(ui, detail, &egui::FontId::proportional(12.0), inner.width());
        ui.painter().text(
            inner.left_top() + egui::vec2(0.0, 18.0),
            egui::Align2::LEFT_TOP,
            detail,
            egui::FontId::proportional(12.0),
            t::sub(),
        );
    } else {
        painter.text(
            egui::pos2(inner.center().x, inner.top()),
            egui::Align2::CENTER_TOP,
            percent.as_deref().unwrap_or("…"),
            egui::FontId::proportional(12.0),
            t::ink(),
        );
    }
    let bar = egui::Rect::from_min_max(
        egui::pos2(inner.left(), inner.bottom() - 3.0),
        inner.right_bottom(),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    motion::progress_bar(
        &mut child,
        fraction.unwrap_or(0.0),
        fraction.is_none(),
        ui.input(|i| i.time),
    );
    if fraction.is_none() && !motion::reduced() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
    response
}

/// 侧边栏的一级入口：宽窗口带文字，窄窗口只有图标。
/// `draw_active_bg` 为 false 时，激活底色由外层弹簧指示器统一绘制以实现丝滑滑行。
pub(crate) fn nav_row_styled(
    ui: &mut egui::Ui,
    height: f32,
    active: bool,
    enabled: bool,
    draw_active_bg: bool,
) -> (egui::Rect, egui::Response, egui::Color32) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hover = motion::hover_amount(ui, response.id.with("hover"), enabled && response.hovered());
    if active && draw_active_bg {
        backdrop::paint_nav_active(ui.ctx(), ui.painter(), rect, motion::R + 1);
        active_accent_bar(ui.painter(), rect);
    } else if !active && hover > 0.0 {
        ui.painter()
            .rect_filled(rect, motion::R + 1, t::tint(t::wash(), 0.72 * hover));
    }
    let color = if !enabled {
        t::dim()
    } else if active {
        t::ink()
    } else {
        motion::mix(t::sub(), t::ink(), hover)
    };
    (rect, response, color)
}

pub(crate) fn nav_row(
    ui: &mut egui::Ui,
    height: f32,
    active: bool,
    enabled: bool,
) -> (egui::Rect, egui::Response, egui::Color32) {
    nav_row_styled(ui, height, active, enabled, true)
}

/// 侧边栏的一级入口：宽窗口带文字，窄窗口只有图标。
pub(crate) fn sidebar_item(
    ui: &mut egui::Ui,
    nav: Nav,
    active: bool,
    wide: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, response, color) = nav_row_styled(ui, 38.0, active, enabled, false);
    let icon = if wide {
        egui::pos2(rect.left() + 22.0, rect.center().y)
    } else {
        rect.center()
    };
    let icon_color = if active && enabled {
        t::accent()
    } else {
        color
    };
    nav_glyph(ui.painter(), icon, nav, icon_color);
    if wide {
        ui.painter().text(
            egui::pos2(rect.left() + 41.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            nav.label(),
            egui::FontId::new(
                14.0,
                if active {
                    bold_family()
                } else {
                    egui::FontFamily::Proportional
                },
            ),
            color,
        );
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response.on_hover_text("任务进行中，完成后再切换")
    }
    .on_hover_text(if wide { nav.hint() } else { nav.label() })
}

pub(crate) fn nav_glyph(painter: &egui::Painter, c: egui::Pos2, nav: Nav, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    let p = |x: f32, y: f32| c + egui::vec2(x, y);
    match nav {
        Nav::Home => {
            painter.add(egui::Shape::closed_line(
                vec![p(-4.5, -7.0), p(7.0, 0.0), p(-4.5, 7.0)],
                stroke,
            ));
        }
        Nav::Versions => {
            for y in [-4.0, 0.0, 4.0] {
                painter.add(egui::Shape::closed_line(
                    vec![p(0.0, y - 3.5), p(7.5, y), p(0.0, y + 3.5), p(-7.5, y)],
                    stroke,
                ));
            }
        }
        Nav::Content => {
            for (x, y) in [(-7.0, -7.0), (1.0, -7.0), (-7.0, 1.0), (1.0, 1.0)] {
                painter.rect_stroke(
                    egui::Rect::from_min_size(p(x, y), egui::vec2(6.0, 6.0)),
                    1.0,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Nav::Shots => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(15.0, 12.0)),
                1.5,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.add(egui::Shape::line(
                vec![
                    p(-5.5, 4.0),
                    p(-2.0, 0.0),
                    p(0.8, 2.6),
                    p(2.6, 1.0),
                    p(5.5, 4.0),
                ],
                stroke,
            ));
            painter.circle_filled(p(3.6, -2.4), 1.1, color);
        }
        Nav::Saves => {
            painter.circle_stroke(c, 7.5, stroke);
            painter.line_segment([p(-7.5, 0.0), p(7.5, 0.0)], stroke);
            painter.add(egui::Shape::ellipse_stroke(c, egui::vec2(3.2, 7.5), stroke));
        }
    }
}

pub(crate) fn sidebar_account(
    ui: &mut egui::Ui,
    account: Option<&nailong_core::accounts::Account>,
    face: Option<&egui::TextureHandle>,
    wide: bool,
) -> egui::Response {
    let h = if wide { 44.0 } else { 38.0 };
    let (rect, response, color) = nav_row(ui, h, false, true);
    let name = account.map(|a| a.name.as_str()).unwrap_or("添加账户");
    let center = if wide {
        egui::pos2(rect.left() + 21.0, rect.center().y)
    } else {
        rect.center()
    };
    let icon = egui::Rect::from_center_size(center, egui::vec2(26.0, 26.0));
    draw_avatar(ui.painter(), icon, name, face);
    ui.painter().rect_stroke(
        icon,
        6,
        egui::Stroke::new(1.0, t::glass_border()),
        egui::StrokeKind::Inside,
    );
    if account.is_some_and(|a| a.stale) {
        ui.painter().circle(
            icon.right_top(),
            3.8,
            t::warn(),
            egui::Stroke::new(1.5, t::rail()),
        );
    }
    if wide {
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y - 7.5),
            egui::Align2::LEFT_CENTER,
            short_text(name, 14),
            egui::FontId::new(13.5, bold_family()),
            color,
        );
        let sub = match account {
            Some(a) if a.stale => "需要重新登录".to_string(),
            Some(a) => a.kind_label(),
            None => "点击登录".to_string(),
        };
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y + 8.5),
            egui::Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(11.5),
            if account.is_some_and(|a| a.stale) {
                t::warn()
            } else {
                t::dim()
            },
        );
    }
    let hint = match account {
        Some(a) if a.stale => format!("{} · 需要重新登录", a.name),
        Some(a) => format!("{} · {}", a.name, a.kind_label()),
        None => "添加账户".into(),
    };
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// 侧边栏底部的工具入口：设置不是一级页面，做成安静的滑杆图标。
pub(crate) fn sidebar_settings(ui: &mut egui::Ui, wide: bool, enabled: bool) -> egui::Response {
    let (rect, response, color) = nav_row(ui, 36.0, false, enabled);
    let center = if wide {
        egui::pos2(rect.left() + 21.0, rect.center().y)
    } else {
        rect.center()
    };
    let stroke = egui::Stroke::new(1.4, color);
    for (y, x) in [(-4.0, -2.5), (0.0, 3.0), (4.0, -0.5)] {
        ui.painter().line_segment(
            [center + egui::vec2(-7.5, y), center + egui::vec2(7.5, y)],
            stroke,
        );
        ui.painter()
            .circle(center + egui::vec2(x, y), 2.0, t::bg(), stroke);
    }
    if wide {
        ui.painter().text(
            egui::pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "设置",
            egui::FontId::proportional(14.0),
            color,
        );
    }
    if enabled {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("设置：外观、内存、Java、目录与日志（Ctrl+,）")
    } else {
        response.on_hover_text("任务进行中")
    }
}
