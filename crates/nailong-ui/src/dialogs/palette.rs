//! 命令面板（`Ctrl+K`）：打几个字，直接到一个版本、一个页面、一个世界或一件常做的事。
//!
//! 启动器里的东西越来越多，这里是「不用记在哪」的那条路：版本名、世界名、设置里的词都能搜。
//! ↑ ↓ 选，Enter 执行，版本上 `Ctrl+Enter` 直接启动，Esc 关掉。

use crate::dialogs::settings::SettingsTab;
use crate::prelude::*;

/// 一次最多列出这么多条，够看也够键盘翻。
const SHOWN: usize = 9;

#[derive(Clone)]
enum Action {
    /// 切到这个版本，回启动台。
    Select(String),
    /// 直接启动这个版本。
    Launch(String),
    /// 启动当前版本并进这个世界。
    EnterWorld(String, String),
    /// 到存档页看这个世界。
    ShowWorld(String, String),
    Go(Nav),
    Install,
    Discover,
    Settings(SettingsTab),
    OpenFolder(PathBuf),
    Log,
    CheckContentUpdates,
    ImportPack,
    Export,
    Duplicate,
    LauncherUpdate,
    AddAccount,
    SwitchAccount(String),
    Theme(t::Preference),
    LaunchParams,
    RestorePoints,
    /// 在已安装的内容里找输入的词。
    FindInstalled,
    /// 去「发现」里搜输入的词。
    SearchOnline,
}

struct Item {
    title: String,
    detail: String,
    /// 右边的小字：这是个什么。
    kind: &'static str,
    /// 另外能搜到它的词（英文、别名）。
    keywords: &'static str,
    action: Action,
    /// 版本行：`Ctrl+Enter` 直接启动。
    launchable: Option<String>,
}

impl Item {
    fn new(title: impl Into<String>, kind: &'static str, action: Action) -> Self {
        Self {
            title: title.into(),
            detail: String::new(),
            kind,
            keywords: "",
            action,
            launchable: None,
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    fn keys(mut self, keywords: &'static str) -> Self {
        self.keywords = keywords;
        self
    }

    /// 和输入的词有多合：名字开头就是它最好，名字里有其次，说明和别名里有再次；不合是 None。
    fn score(&self, words: &[String]) -> Option<u32> {
        let title = self.title.to_lowercase();
        let rest = format!("{} {} {}", self.detail, self.keywords, self.kind).to_lowercase();
        let mut total = 0;
        for word in words {
            total += if title.starts_with(word.as_str()) {
                0
            } else if title.contains(word.as_str()) {
                1
            } else if rest.contains(word.as_str()) {
                3
            } else {
                return None;
            };
        }
        Some(total)
    }
}

pub(crate) struct Palette {
    query: String,
    items: Vec<Item>,
    cursor: usize,
    /// 刚打开：把焦点给输入框。
    fresh: bool,
}

impl App {
    pub(crate) fn open_palette(&mut self) {
        self.login.open = false;
        self.palette = Some(Palette {
            query: String::new(),
            items: self.palette_items(),
            cursor: 0,
            fresh: true,
        });
    }

    /// 现在能做的事。打开面板时列一次：版本、当前版本的世界、页面、常用操作、账户。
    fn palette_items(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        let current = self.selected_row();
        // 版本：最近玩过的在前（列表本来就是这个顺序）。
        for row in &self.rows {
            let mut item = Item::new(row.id.clone(), "版本", Action::Select(row.id.clone()))
                .detail(match self.session(&row.id) {
                    Some(session) => format!("{} · {}", row_kind(row), session.status()),
                    None => row_kind(row),
                });
            if row.health.missing == 0 && !self.is_running(&row.id) {
                item.launchable = Some(row.id.clone());
            }
            items.push(item);
        }
        if let Some(row) = &current {
            if row.health.missing == 0 && !self.is_running(&row.id) {
                items.push(
                    Item::new("开始游戏", "操作", Action::Launch(row.id.clone()))
                        .detail(row.id.clone())
                        .keys("play launch start 启动"),
                );
            }
            // 当前版本的世界：能直接进的直接进，不能的去存档页。
            let quick = self.quick_play_ok(row) && !self.is_running(&row.id);
            let mut worlds = saves::worlds(&row.game_dir).unwrap_or_default();
            worlds.truncate(30);
            for world in worlds {
                let action = if quick {
                    Action::EnterWorld(row.id.clone(), world.name.clone())
                } else {
                    Action::ShowWorld(row.id.clone(), world.name.clone())
                };
                items.push(
                    Item::new(world.name.clone(), "世界", action)
                        .detail(if quick {
                            format!("开始游戏并进入 · {}", human_age(world.modified))
                        } else {
                            format!("快照和备份 · {}", human_age(world.modified))
                        })
                        .keys("world save 存档"),
                );
            }
        }
        for nav in Nav::ALL {
            items.push(
                Item::new(nav.label(), "页面", Action::Go(nav))
                    .detail(nav.hint())
                    .keys(match nav {
                        Nav::Home => "home",
                        Nav::Versions => "versions instances",
                        Nav::Content => "mods content resourcepacks shaders 模组",
                        Nav::Saves => "saves worlds backups 世界 备份 快照",
                        Nav::Shots => "screenshots gallery",
                    }),
            );
        }
        items.push(
            Item::new("安装新版本", "操作", Action::Install)
                .detail("原版、Fabric、NeoForge、Forge")
                .keys("install new minecraft 下载 游戏"),
        );
        items.push(
            Item::new("导入整合包", "操作", Action::ImportPack)
                .detail(".mrpack / .zip")
                .keys("modpack import curseforge modrinth"),
        );
        items.push(
            Item::new("发现内容", "页面", Action::Discover)
                .detail("Mod、资源包、光影")
                .keys("browse discover search modrinth curseforge 下载 模组"),
        );
        if let Some(row) = &current {
            items.push(
                Item::new("检查内容更新", "操作", Action::CheckContentUpdates)
                    .detail(row.id.clone())
                    .keys("update mods 更新 模组"),
            );
            items.push(
                Item::new("恢复点", "操作", Action::RestorePoints)
                    .detail("Mod 和配置回到之前的状态")
                    .keys("rollback restore 回滚 退回"),
            );
            items.push(
                Item::new("启动参数", "操作", Action::LaunchParams)
                    .detail("Java、内存、窗口")
                    .keys("jvm memory java args 参数"),
            );
            items.push(
                Item::new(
                    "打开版本文件夹",
                    "操作",
                    Action::OpenFolder(row.game_dir.clone()),
                )
                .detail(row.id.clone())
                .keys("folder directory open 目录"),
            );
            items.push(
                Item::new("复制这个版本", "操作", Action::Duplicate)
                    .detail("试新 Mod 前留一份")
                    .keys("duplicate copy clone 克隆"),
            );
            items.push(
                Item::new("导出整合包", "操作", Action::Export)
                    .detail(row.id.clone())
                    .keys("export mrpack share 分享"),
            );
        }
        items.push(
            Item::new("查看日志", "操作", Action::Log)
                .detail("游戏输出和启动器记录")
                .keys("log crash 崩溃 报错"),
        );
        items.push(
            Item::new(
                "打开数据目录",
                "操作",
                Action::OpenFolder(self.root.dir().to_path_buf()),
            )
            .keys("folder directory .minecraft 目录"),
        );
        for (tab, keys) in [
            (SettingsTab::Game, "settings java memory gc jvm 内存 回收器"),
            (
                SettingsTab::Launcher,
                "settings theme zoom 外观 主题 缩放 动效 下载源 镜像",
            ),
            (SettingsTab::Storage, "settings folder clean 目录 清理 空间"),
            (
                SettingsTab::About,
                "settings about update version 更新 版本 快捷键",
            ),
        ] {
            items.push(
                Item::new(
                    format!("设置 › {}", tab.label()),
                    "设置",
                    Action::Settings(tab),
                )
                .detail(tab.hint())
                .keys(keys),
            );
        }
        items.push(
            Item::new("检查启动器更新", "操作", Action::LauncherUpdate)
                .detail(format!("现在是 v{}", env!("CARGO_PKG_VERSION")))
                .keys("update upgrade cairn 更新"),
        );
        for pref in t::Preference::ALL {
            if pref != self.prefs.theme {
                items.push(
                    Item::new(
                        format!("外观：{}", pref.label()),
                        "设置",
                        Action::Theme(pref),
                    )
                    .keys("theme dark light 主题 深色 浅色"),
                );
            }
        }
        let current_account = self.accounts.current.clone();
        for account in &self.accounts.list {
            if Some(&account.id) != current_account.as_ref() {
                items.push(
                    Item::new(
                        format!("换成 {}", account.name),
                        "账户",
                        Action::SwitchAccount(account.id.clone()),
                    )
                    .detail(account.kind_label())
                    .keys("account switch 账号 切换"),
                );
            }
        }
        items.push(
            Item::new("添加账户", "账户", Action::AddAccount)
                .detail("Microsoft、外置登录、离线")
                .keys("account login microsoft 登录 账号"),
        );
        items
    }

    pub(crate) fn palette_overlay(&mut self, ctx: &egui::Context) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let words: Vec<String> = palette
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        // 合适的排前面；一样合适的保持原来的顺序（版本在最前）。
        let mut matched: Vec<(u32, usize)> = palette
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.score(&words).map(|score| (score, index)))
            .collect();
        matched.sort_by_key(|(score, index)| (*score, *index));
        let mut shown: Vec<usize> = matched.into_iter().map(|(_, index)| index).collect();
        // 输了词：再给两条「拿这个词去搜」，放在最后。
        let typed = palette.query.trim().to_string();
        let has_target = self.selected.is_some();
        let extra: Vec<Item> = if typed.is_empty() || !has_target {
            Vec::new()
        } else {
            vec![
                Item::new(
                    format!("在已安装的内容里找「{typed}」"),
                    "内容",
                    Action::FindInstalled,
                ),
                Item::new(
                    format!("去发现里搜「{typed}」"),
                    "内容",
                    Action::SearchOnline,
                ),
            ]
        };
        let total = shown.len() + extra.len();
        palette.cursor = palette.cursor.min(total.saturating_sub(1));

        let (up, down, enter, launch, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if total > 0 {
            if down {
                palette.cursor = (palette.cursor + 1) % total;
            }
            if up {
                palette.cursor = (palette.cursor + total - 1) % total;
            }
        }

        // 光标所在的那一条要在显示的窗口里。
        let first = palette
            .cursor
            .saturating_sub(SHOWN - 1)
            .min(total.saturating_sub(SHOWN));
        let mut run: Option<Action> = None;
        let mut clicked_outside = false;
        let screen = ctx.content_rect();
        let width = (screen.width() - 64.0).clamp(360.0, 580.0);
        let id = egui::Id::unique("palette");
        let modal = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(
                egui::Align2::CENTER_TOP,
                [0.0, (screen.height() * 0.14).max(48.0)],
            ))
            .backdrop_color(t::veil(0.7))
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 10)),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 2.0;
                let before = palette.query.clone();
                let field = ui.add(
                    egui::TextEdit::singleline(&mut palette.query)
                        .hint_text("找版本、世界、页面、设置，或者要做的事")
                        .desired_width(width - 20.0)
                        .font(egui::FontId::proportional(16.0))
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 9))),
                );
                if palette.fresh || !field.has_focus() {
                    field.request_focus();
                    palette.fresh = false;
                }
                if palette.query != before {
                    palette.cursor = 0;
                }
                let line = ui.cursor().top() + 2.0;
                ui.painter().line_segment(
                    [
                        egui::pos2(ui.max_rect().left(), line),
                        egui::pos2(ui.max_rect().right(), line),
                    ],
                    egui::Stroke::new(1.0, t::hairline()),
                );
                ui.add_space(8.0);
                if total == 0 {
                    ui.add_space(14.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("没有找到").size(13.0).color(t::dim()));
                    });
                    ui.add_space(14.0);
                }
                for position in first..(first + SHOWN).min(total) {
                    let item = match shown.get(position) {
                        Some(&index) => &palette.items[index],
                        None => &extra[position - shown.len()],
                    };
                    let response = palette_row(ui, item, position == palette.cursor);
                    if response.clicked() {
                        run = Some(item.action.clone());
                    }
                    // 鼠标动过才跟着鼠标走，不然键盘选的会被停在上面的鼠标抢走。
                    if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        palette.cursor = position;
                    }
                }
                ui.add_space(6.0);
                let launchable = shown
                    .get(palette.cursor)
                    .is_some_and(|&index| palette.items[index].launchable.is_some());
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let hint = if launchable {
                        "↑ ↓ 选择 · Enter 切换到它 · Ctrl+Enter 直接启动 · Esc 关闭"
                    } else {
                        "↑ ↓ 选择 · Enter 执行 · Esc 关闭"
                    };
                    ui.label(egui::RichText::new(hint).size(11.5).color(t::dim()));
                    if total > SHOWN {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("{} / {total}", palette.cursor + 1))
                                    .size(11.5)
                                    .color(t::dim()),
                            );
                        });
                    }
                });
            });
        if modal.backdrop_response.clicked() {
            clicked_outside = true;
        }
        if total > 0 && (enter || launch) {
            let item = match shown.get(palette.cursor) {
                Some(&index) => &palette.items[index],
                None => &extra[palette.cursor - shown.len()],
            };
            run = Some(match (&item.launchable, launch) {
                (Some(id), true) => Action::Launch(id.clone()),
                _ => item.action.clone(),
            });
        }
        shown.clear();
        if let Some(action) = run {
            self.palette = None;
            self.run_palette(ctx, action, &typed);
        } else if escape || clicked_outside {
            self.palette = None;
        }
    }

    fn run_palette(&mut self, ctx: &egui::Context, action: Action, typed: &str) {
        match action {
            Action::Select(id) => {
                self.selected = Some(id);
                self.nav_go(Nav::Home);
                self.progress_open = false;
                self.page = Page::Home;
            }
            Action::Launch(id) => {
                if let Some(row) = self.row(&id) {
                    self.launch_row(&row, None);
                }
            }
            Action::EnterWorld(id, world) => {
                if let Some(row) = self.row(&id) {
                    self.launch_row(&row, Some(launch::QuickPlay::World(world)));
                }
            }
            Action::ShowWorld(id, world) => {
                if let Some(row) = self.row(&id) {
                    self.open_saves(row);
                    self.worlds.selected = Some(world);
                }
            }
            Action::Go(nav) => {
                // 面板里选「内容」就是去内容页，不是「再点一次回到已安装」。
                self.progress_open = false;
                if self.page.nav() != nav || self.progress_showing() {
                    self.nav_go(nav);
                }
            }
            Action::Install => self.open_install(),
            Action::Discover => self.open_content(false),
            Action::Settings(tab) => {
                self.settings_tab = tab;
                self.open_settings();
            }
            Action::OpenFolder(dir) => {
                let _ = std::fs::create_dir_all(&dir);
                open_folder(&dir);
            }
            Action::Log => self.open_log(),
            Action::CheckContentUpdates => {
                self.open_content(true);
                self.content.status = StatusFilter::All;
                self.content.pending_update_check = true;
            }
            Action::ImportPack => {
                if let Some(path) = pick_modpack() {
                    self.start_modpack(path);
                }
            }
            Action::Export => {
                if let Some(row) = self.selected_row() {
                    self.open_export(&row);
                }
            }
            Action::Duplicate => {
                if let Some(row) = self.selected_row() {
                    self.open_duplicate(&row);
                }
            }
            Action::LauncherUpdate => {
                self.settings_tab = SettingsTab::About;
                self.open_settings();
                self.check_update(ctx, true);
            }
            Action::AddAccount => self.open_add_account(AddTab::Microsoft),
            Action::SwitchAccount(id) => self.select_account(&id),
            Action::Theme(pref) => {
                self.prefs.theme = pref;
                pref.apply(ctx);
                self.save_ui_config();
            }
            Action::LaunchParams => {
                if let Some(row) = self.selected_row() {
                    self.open_launch_edit(&row);
                }
            }
            Action::RestorePoints => {
                self.open_content(true);
                self.open_rollback();
            }
            Action::FindInstalled => {
                self.open_content(true);
                self.content.status = StatusFilter::All;
                self.content.filter = typed.to_string();
            }
            Action::SearchOnline => {
                self.open_content(false);
                self.content.query = typed.to_string();
                self.start_mods_search();
            }
        }
    }
}

/// 面板里的一行：左边名字和说明，右边一个小字说明这是什么。
fn palette_row(ui: &mut egui::Ui, item: &Item, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
    if active {
        ui.painter().rect_filled(rect, motion::R, t::wash());
    }
    let kind_font = egui::FontId::proportional(11.5);
    let kind = ui
        .painter()
        .layout_no_wrap(item.kind.to_string(), kind_font, t::dim());
    let kind_w = kind.size().x;
    ui.painter().galley(
        egui::pos2(
            rect.right() - 12.0 - kind_w,
            rect.center().y - kind.size().y * 0.5,
        ),
        kind,
        t::dim(),
    );
    let left = rect.left() + 12.0;
    let room = rect.width() - 24.0 - kind_w - 12.0;
    let title_font = egui::FontId::proportional(14.0);
    let title = elide_middle(ui, &item.title, &title_font, room);
    let title = ui.painter().layout_no_wrap(title, title_font, t::ink());
    let title_w = title.size().x;
    ui.painter().galley(
        egui::pos2(left, rect.center().y - title.size().y * 0.5),
        title,
        t::ink(),
    );
    let detail_room = room - title_w - 12.0;
    if !item.detail.is_empty() && detail_room > 40.0 {
        let font = egui::FontId::proportional(12.5);
        let detail = elide_middle(ui, &item.detail, &font, detail_room);
        ui.painter().text(
            egui::pos2(left + title_w + 12.0, rect.center().y + 0.5),
            egui::Align2::LEFT_CENTER,
            detail,
            font,
            t::dim(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn names_beat_descriptions_and_every_word_must_match() {
        let version = Item::new("生存 1.21", "版本", Action::Select("生存 1.21".into()))
            .detail("1.21.1 · NeoForge 21.1.209");
        let settings = Item::new("设置 › 游戏", "设置", Action::Settings(SettingsTab::Game))
            .keys("settings java memory");
        // 名字开头 < 名字里有 < 说明或别名里有。
        assert_eq!(version.score(&words("生存")), Some(0));
        assert_eq!(version.score(&words("1.21")), Some(1));
        assert_eq!(version.score(&words("neoforge")), Some(3));
        assert_eq!(settings.score(&words("JAVA")), Some(3));
        // 两个词都要对得上。
        assert_eq!(version.score(&words("生存 neoforge")), Some(3));
        assert_eq!(version.score(&words("生存 fabric")), None);
        // 没输入时什么都算合适。
        assert_eq!(settings.score(&[]), Some(0));
    }
}
