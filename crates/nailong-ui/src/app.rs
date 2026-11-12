//! 应用的骨架：全部状态、启动时的初始化，以及每一帧的流程（收消息 → 快捷键 → 侧栏 → 页面 → 弹窗）。

use crate::prelude::*;

impl App {
    pub(crate) fn new(root: Root, ctx: &egui::Context) -> Self {
        let javas = java::find_all(&root);
        let java_ok = javas.iter().any(|j| j.major >= 21);
        let java_label = javas.iter().find(|j| j.major >= 21).map(|j| j.label());
        let backup_cfg = snapshots::load_config(&root);
        let prefs = UiPrefs::load(&root);
        let accounts = load_accounts(&root, &prefs.username);
        let (icon_jobs, icon_job_rx) = mpsc::channel::<(String, String)>();
        let (icon_done, icon_rx) = mpsc::channel::<(String, Vec<u8>)>();
        std::thread::spawn(move || {
            while let Ok((key, url)) = icon_job_rx.recv() {
                if let Ok(bytes) = mods::fetch_icon(&url)
                    && icon_done.send((key, bytes)).is_err()
                {
                    break;
                }
            }
        });
        let mut app = Self {
            play: PlayState {
                tuning_cache: HashMap::new(),
                versions: BTreeMap::new(),
                last: None,
                sessions: Vec::new(),
                stop_pending: Vec::new(),
                stop_confirm: None,
                stats: nailong_core::played::load(&root),
                recovery: None,
                recovery_open: false,
                minimized: false,
                launch_after_java: None,
            },
            env: Env {
                javas,
                extra_javas: Vec::new(),
                java_ok,
                java_label,
                machine: tuning::Machine::detect(),
                bench: None,
            },
            home: HomeState {
                cache: None,
                scan: None,
                verified: None,
                quick_play: HashMap::new(),
                switch_query: String::new(),
            },
            selected: prefs.selected.clone(),
            prefs,
            ctx: ctx.clone(),
            install: InstallForm {
                catalog: None,
                catalog_loading: false,
                catalog_error: None,
                catalog_rx: None,
                mc: "1.21.1".into(),
                kind: LoaderKind::Fabric,
                fabric: "0.19.5".into(),
                neoforge: "21.1.209".into(),
                forge: "52.1.20".into(),
                name: String::new(),
                name_edited: false,
                repair_target: None,
            },
            export: Default::default(),
            data_dir: Default::default(),
            rollback: RollbackState {
                open: false,
                points: Vec::new(),
                busy: false,
                confirm: None,
                notice: None,
                error: None,
                rx: None,
            },
            login: LoginState::default(),
            worlds: WorldsState {
                row: None,
                selected: None,
                list: Vec::new(),
                confirm: None,
                confirm_backup: None,
                snapshots: HashMap::new(),
                rx: None,
                busy: false,
                notice: None,
                error: None,
                backup: backup_cfg,
                auto_backup_running: false,
            },
            content: ContentState {
                row: None,
                game_dir: None,
                query: String::new(),
                filter: String::new(),
                status: StatusFilter::All,
                recent: HashSet::new(),
                installed_dir: None,
                dir_sig: 0,
                delete_confirm: None,
                kind: mods::ContentKind::Mod,
                browse: Vec::new(),
                browse_loading: false,
                category: None,
                source_filter: None,
                mc: None,
                loader: None,
                chips_expanded: false,
                shelf: None,
                searching: false,
                quick: None,
                installed_keys: HashSet::new(),
                show_installed: false,
                installed_loading: false,
                installed: Vec::new(),
                updates_loading: false,
                updates: Vec::new(),
                results: Vec::new(),
                total: 0,
                offset: 0,
                warnings: Vec::new(),
                loading: false,
                selected: None,
                versions: Vec::new(),
                versions_loading: false,
                version: None,
                choices: Vec::new(),
                optional: BTreeSet::new(),
                deps_loading: false,
                notice: None,
                error: None,
                rx: None,
                scan_rx: None,
                icons: HashMap::new(),
                icons_pending: HashSet::new(),
                cf_categories: HashMap::new(),
                cf_loading: HashSet::new(),
                cf_rx: None,
                icon_jobs,
                icon_rx,
                focus_search: false,
                drop_preview: None,
                pending_update_check: false,
            },
            root,
            rows: Vec::new(),
            show_tuning: false,
            bench_confirm: false,
            launch_edit: None,
            page: Page::Home,
            palette: None,
            duplicate: None,
            notice_seen: None,
            delete_confirm: None,
            renaming: None,
            delete_failed: None,
            progress_open: false,
            progress_seen: None,
            cleanup: Default::default(),
            window_focused: true,
            busy: None,
            cancel_requested: false,
            activity: None,
            update: UpdateState::default(),
            error: None,
            notice: None,
            log: vec!["启动器已就绪".into()],
            log_view: None,
            gallery: Default::default(),
            accounts,
            show_settings: false,
            pending_install: None,
            version_cache: None,
            page_key: 255,
            page_enter_at: 0.0,
            version_query: String::new(),
            version_search_focus: false,
            settings_tab: Default::default(),
            rx: None,
        };
        app.play.versions = launchcfg::load_all(&app.root);
        app.refresh();
        app.env.bench = bench::load(&app.root);
        {
            // 0.35 给每个版本复制过一份测试实例，现在用不到了。
            let root = app.root.clone();
            std::thread::spawn(move || bench::cleanup_legacy(&root));
        }
        #[cfg(debug_assertions)]
        if let Ok(theme) = std::env::var("NAILONG_DEBUG_THEME") {
            app.prefs.theme = t::Preference::from_key(&theme);
        }
        // 调试钩子：跳过点击直接进下载中心，方便无头冒烟测试（仅 debug 构建）。
        #[cfg(debug_assertions)]
        if let Ok(id) = std::env::var("NAILONG_DEBUG_SELECT")
            && let Some(row) = app.rows.iter().find(|row| row.id.contains(&id))
        {
            app.selected = Some(row.id.clone());
        }
        #[cfg(debug_assertions)]
        if let Some(screen) = std::env::var_os("NAILONG_DEBUG_SCREEN") {
            let screen = screen.to_string_lossy().to_string();
            let row = app.selected_row().or_else(|| app.rows.first().cloned());
            match screen.as_str() {
                "install" => app.open_install(),
                "manage" => app.page = Page::Versions,
                "export" | "export-done" => {
                    if let Some(row) = &row {
                        app.open_export(row);
                        if screen == "export-done" {
                            app.export.result = Some(Ok(nailong_core::export::Report {
                                path: PathBuf::from(
                                    "/home/player/下载/新：模拟大都市-1.0.0.mrpack",
                                ),
                                online: 150,
                                packed: 6254,
                                bytes: 322_000_000,
                                not_found: vec![
                                    "mods/aegis-1.4.0.jar".into(),
                                    "mods/waimailaile-1.0.8.jar".into(),
                                    "resourcepacks/新模拟大都市音频.zip".into(),
                                ],
                                warnings: Vec::new(),
                            }));
                        }
                    }
                }
                "datadir" => app.open_data_dir(),
                "rename" | "delete" => {
                    app.page = Page::Versions;
                    if let Some(row) = &row {
                        app.selected = Some(row.id.clone());
                        if screen == "rename" {
                            app.renaming = Some((row.id.clone(), format!("{} 生存", row.id)));
                        } else {
                            app.delete_confirm = Some(row.id.clone());
                        }
                    }
                }
                "mods" => {
                    if row.is_some() {
                        app.content.row = row.clone();
                        app.open_mods();
                        // Preload the installed index so the tab count and
                        // "已装" badges match what the player sees after
                        // visiting the installed tab once.
                        app.start_mods_scan();
                    }
                }
                "saves" => {
                    if let Some(row) = row {
                        app.open_saves(row);
                    }
                }
                "gallery" | "gallery-time" | "gallery-places" | "gallery-people" => {
                    use crate::pages::gallery::GalleryTab;
                    app.open_gallery(None);
                    app.gallery.tab = match screen.as_str() {
                        "gallery-time" => GalleryTab::Time,
                        "gallery-places" => GalleryTab::Places,
                        "gallery-people" => GalleryTab::People,
                        _ => GalleryTab::Overview,
                    };
                }
                "installed" => {
                    if let Some(row) = row {
                        app.content.row = Some(row);
                        app.open_mods();
                        app.content.show_installed = true;
                        app.start_mods_scan();
                    }
                }
                "account" | "accounts" | "add-account" | "add-external" | "add-offline" => {
                    use nailong_core::accounts::{Account, Accounts, Login};
                    let mut accounts = Accounts::default();
                    let mut steve = Account::from_microsoft(auth::Account {
                        name: "Steve".into(),
                        uuid: "069a79f444e94726a5befca90e38aaf5".into(),
                        access_token: "debug".into(),
                        refresh_token: "debug".into(),
                        expires_at: u64::MAX / 2,
                        xuid: None,
                        client_id: String::new(),
                        skin: None,
                    });
                    steve.skin = std::env::var("NAILONG_DEBUG_SKIN").ok();
                    let steve_id = accounts.add(steve);
                    let mut external = Account::offline("dk_skin");
                    external.login = Login::External {
                        api: "https://littleskin.cn/api/yggdrasil/".into(),
                        server_name: "LittleSkin".into(),
                        login: "dk@example.com".into(),
                        access_token: "debug".into(),
                        client_token: "debug".into(),
                    };
                    accounts.add(external);
                    let mut stale = Account::offline("BuilderZ");
                    stale.login = Login::External {
                        api: "https://skin.example.com/api/yggdrasil/".into(),
                        server_name: "朋友的皮肤站".into(),
                        login: "builder".into(),
                        access_token: "debug".into(),
                        client_token: "debug".into(),
                    };
                    stale.stale = true;
                    accounts.add(stale);
                    accounts.add(Account::offline("dkjsiogu"));
                    accounts.select(&steve_id);
                    app.accounts = accounts;
                    match screen.as_str() {
                        "add-account" => app.open_add_account(AddTab::Microsoft),
                        "add-external" => {
                            app.open_add_account(AddTab::External);
                            if let (Some(form), Ok(server)) =
                                (&mut app.login.add, std::env::var("NAILONG_DEBUG_SERVER"))
                            {
                                form.server_input = server;
                                form.check_now = true;
                            }
                        }
                        "add-offline" => app.open_add_account(AddTab::Offline),
                        _ => app.login.open = true,
                    }
                }
                "login" => {
                    app.login.url = Some(
                        "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize".into(),
                    );
                    app.login.busy = true;
                }
                "login_error" => {
                    app.login.open = true;
                    app.login.error = Some("登录参数不完整，请更新到最新版本后重试".into());
                }
                "settings" => {
                    app.show_settings = true;
                }
                "log" => {
                    if let Some(row) = row {
                        app.play.last = Some(log::LastLaunch {
                            label: row.label.clone(),
                            log: launch::log_path(&app.root, &row.profile),
                            game_dir: row.game_dir.clone(),
                            started: std::time::SystemTime::now() - Duration::from_secs(180),
                            outcome: log::Outcome::Exited(1),
                        });
                        app.error =
                            Some("游戏异常退出（代码 1）：缺少前置 Mod：日志里写了缺哪个".into());
                        app.open_log();
                    }
                }
                "java" => app.start_java_download(17, None, None),
                screen if screen.starts_with("update") => {
                    app.update.debug(screen);
                    app.show_settings = screen.starts_with("update-settings");
                    // 别让启动时的自动检查把这个假版本冲掉。
                    app.prefs.update_checked = u64::MAX / 2;
                }
                "running" | "running-versions" | "stop" => {
                    if let Some(row) = &row {
                        app.selected = Some(row.id.clone());
                        app.play.sessions.push(crate::play::Session::debug(row));
                        if let Some(other) = app.rows.iter().find(|other| other.id != row.id) {
                            app.play.sessions.push(crate::play::Session::debug(other));
                        }
                        if screen == "running-versions" {
                            app.page = Page::Versions;
                        }
                        if screen == "stop" {
                            app.play.stop_confirm = Some(row.id.clone());
                        }
                    }
                }
                "palette" => app.open_palette(),
                "recovery" | "recovery-open" => {
                    if let Some(row) = &row {
                        let now = crate::play::unix_now();
                        let point = |age: u64, reason: &str| rollback::Snapshot {
                            id: format!("debug-{age}"),
                            created_at: now - age,
                            reason: reason.into(),
                            bytes: 48_000_000,
                            files: Vec::new(),
                            extra_files: Vec::new(),
                        };
                        app.play.recovery = Some(crate::play::Recovery {
                            row_id: row.id.clone(),
                            game_dir: row.game_dir.clone(),
                            point: point(7200, "安装 Sodium 前"),
                            changes: 3,
                            last_good: now - 86400 * 2,
                        });
                        app.error = Some(
                            "游戏异常退出（代码 1）：有 Mod 注入失败，通常是 Mod 和游戏版本或其他 Mod 冲突"
                                .into(),
                        );
                        app.play.recovery_open = screen == "recovery-open";
                    }
                }
                "duplicate" => {
                    app.page = Page::Versions;
                    if let Some(row) = &row {
                        app.selected = Some(row.id.clone());
                        app.open_duplicate(row);
                    }
                }
                "notice" => app.notice = Some("已把「1.20.1 Fabric」移到回收站".into()),
                "progress" | "progress-bg" => {
                    app.busy = Some(Task::Install);
                    let mut activity = Activity::new("1.21.1 · NeoForge 21.1.264", "读取版本信息")
                        .expect(crate::activity::plan::FORGE);
                    // 资源和 processor 同时进行，两边交替报。
                    activity.update(Progress {
                        stage: "下载运行库".into(),
                        files_done: 64,
                        files_total: 64,
                        bytes_done: 48_000_000,
                        bytes_total: 48_000_000,
                        ..Default::default()
                    });
                    for (i, name) in [
                        "minecraft/sounds/block/stone/step3.ogg",
                        "minecraft/textures/block/deepslate_bricks.png",
                        "minecraft/lang/zh_cn.json",
                        "minecraft/sounds/ambient/cave/cave13.ogg",
                        "icons/minecraft.icns",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        activity.update(Progress {
                            stage: "下载游戏资源".into(),
                            files_done: 2808 + i as u64,
                            files_total: 3911,
                            bytes_done: 180_000_000 + i as u64 * 1_000_000,
                            bytes_total: 320_000_000,
                            current: Some(name.into()),
                            source: Some("resources.download.minecraft.net".into()),
                            ..Default::default()
                        });
                        activity.update(Progress {
                            stage: "运行 NeoForge 处理器".into(),
                            files_done: 3,
                            files_total: 9,
                            ..Default::default()
                        });
                    }
                    app.activity = Some(activity);
                    if screen == "progress-bg" {
                        // 已经点侧栏离开了进度页：任务在后台继续。
                        app.progress_seen = app.busy;
                        app.progress_open = false;
                    }
                }
                _ => {}
            }
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("NAILONG_DEBUG_OPEN_MODS").is_some()
            && let Some(row) = app.selected_row().or_else(|| app.rows.first().cloned())
        {
            app.content.row = Some(row);
            app.open_mods();
        }
        #[cfg(debug_assertions)]
        if let Ok(term) = std::env::var("NAILONG_DEBUG_PROJECT")
            && app.page == Page::Content
            && app.content.selected.is_none()
            && let Ok(page) = mods::search(
                &term,
                None,
                app.content.mc.as_deref(),
                app.content.loader,
                app.content.kind,
                0,
                8,
            )
            && let Some(project) = page.hits.into_iter().next()
        {
            app.request_icons(std::iter::once(project.clone()));
            app.select_mods_project(project);
        }
        #[cfg(debug_assertions)]
        if let Ok(term) = std::env::var("NAILONG_DEBUG_SEARCH")
            && app.page == Page::Content
        {
            app.content.query = term;
            app.start_mods_search();
        }
        #[cfg(debug_assertions)]
        if std::env::var("NAILONG_DEBUG_REVIEW").as_deref() == Ok("quick")
            && app.page == Page::Content
            && let Some(project) = app.content.selected.clone()
        {
            app.review_quick_install(project);
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("NAILONG_DEBUG_OPEN_SAVES").is_some()
            && let Some(row) = app.selected_row().or_else(|| app.rows.first().cloned())
        {
            app.open_saves(row);
        }
        #[cfg(debug_assertions)]
        if let Some(count) = std::env::var("NAILONG_DEBUG_UPDATES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            && let Some(row) = app.selected_row().or_else(|| app.rows.first().cloned())
        {
            let dir = row.game_dir.clone();
            app.content.updates = installed::load(&dir)
                .entries
                .into_iter()
                .filter(|entry| entry.kind == mods::ContentKind::Mod)
                .take(count)
                .map(|entry| installed::UpdateCandidate {
                    latest: mods::Version {
                        source: mods::Source::Modrinth,
                        kind: mods::ContentKind::Mod,
                        project_id: String::new(),
                        id: String::new(),
                        name: entry.name.clone(),
                        version_number: "debug".into(),
                        game_versions: Vec::new(),
                        loaders: Vec::new(),
                        file_name: entry.file.clone(),
                        download_url: None,
                        sha1: None,
                        size: None,
                        published: None,
                        dependencies: Vec::new(),
                    },
                    entry,
                })
                .collect();
            app.content.row = Some(row);
        }
        #[cfg(debug_assertions)]
        if std::env::var("NAILONG_DEBUG_SCREEN").as_deref() == Ok("welcome") {
            app.page = Page::Home;
        }
        app
    }

    /// 整个界面锁住、只能等的时候：登录中、测这台电脑时。下载和安装不锁，可以照常浏览。
    pub(crate) fn page_locked(&self) -> bool {
        self.login.busy || matches!(self.busy, Some(Task::Bench))
    }

    /// 换到一个页面。只换视图，不动数据；各页面的 `open_*` 负责准备自己的状态。
    pub(crate) fn go(&mut self, page: Page) {
        self.page = page;
        self.progress_open = false;
    }

    /// 现在不能改这个版本的原因；`None` 表示可以改。`row_id` 为 `None` 是问「能不能动整个数据目录」。
    pub(crate) fn write_block(&self, row_id: Option<&str>) -> Option<&'static str> {
        match self.busy {
            Some(Task::Install) | Some(Task::Mods) => return Some("正在安装，完成后再试"),
            Some(Task::Files) => return Some("正在整理内容，完成后再试"),
            Some(Task::Export) => return Some("正在导出整合包，完成后再试"),
            Some(Task::Bench) => return Some("正在测试这台电脑，完成后再试"),
            Some(Task::Java) => return Some("正在下载 Java，完成后再试"),
            None => {}
        }
        match row_id {
            Some(id) if self.is_running(id) => Some("游戏正在使用这个版本，退出游戏后再修改"),
            None if self.any_game() => Some("游戏运行中，退出游戏后再试"),
            _ => None,
        }
    }

    /// 按钮不可用时给出的说明。
    pub(crate) fn busy_reason(&self) -> &'static str {
        match self.busy {
            Some(Task::Bench) => "正在测试这台电脑，完成后再试",
            Some(Task::Java) => "正在下载 Java，完成后再试",
            Some(Task::Export) => "正在导出整合包，完成后再试",
            Some(Task::Files) => "正在整理内容，完成后再试",
            _ => "正在安装，完成后再试",
        }
    }

    /// 有弹窗盖在页面上。快捷键、拖放提示这些只在没有弹窗时生效；新弹窗记得加进来。
    pub(crate) fn modal_open(&self) -> bool {
        self.show_settings
            || self.login.open
            || self.login.busy
            || self.login.url.is_some()
            || self.login.error.is_some()
            || self.login.add.is_some()
            || self.pending_install.is_some()
            || self.rollback.open
            || self.log_view.is_some()
            || self.launch_edit.is_some()
            || self.bench_confirm
            || self.update.open
            || self.export.open
            || self.data_dir.open
            || self.palette.is_some()
            || self.duplicate.is_some()
            || self.play.recovery_open
            || self
                .gallery
                .shots
                .as_ref()
                .is_some_and(|shots| shots.viewing())
    }

    /// 进度页是不是正在整页显示。
    pub(crate) fn progress_showing(&self) -> bool {
        self.busy.is_some_and(Task::shows_progress) && (self.progress_open || self.page_locked())
    }

    /// 当前视图的编号，变了就放一次切页动效。
    pub(crate) fn view_key(&self) -> u8 {
        if self.progress_showing() {
            return 10;
        }
        match self.page {
            Page::Home => 0,
            Page::Install => 1,
            Page::Versions => 2,
            Page::Content if self.content.show_installed => 6,
            Page::Content if self.content.selected.is_some() => 5,
            Page::Content if self.content.searching => 4,
            Page::Content => 3,
            Page::Worlds => 8,
            Page::Shots => 9,
        }
    }

    /// 全局快捷键：只在不输入文字、没有弹窗时生效。`Ctrl+K` 例外，它自己就是开关。
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
            if self.palette.is_some() {
                self.palette = None;
            } else if !self.modal_open() && !self.page_locked() {
                self.open_palette();
            }
            return;
        }
        if ctx.egui_wants_keyboard_input() || self.modal_open() {
            return;
        }
        let mut nav = None;
        let mut settings = false;
        let mut refresh = false;
        let mut search = false;
        let mut logs = false;
        ctx.input_mut(|input| {
            for (key, target) in [
                (egui::Key::Num1, Nav::Home),
                (egui::Key::Num2, Nav::Versions),
                (egui::Key::Num3, Nav::Content),
                (egui::Key::Num4, Nav::Saves),
                (egui::Key::Num5, Nav::Shots),
            ] {
                if input.consume_key(egui::Modifiers::CTRL, key) {
                    nav = Some(target);
                }
            }
            settings = input.consume_key(egui::Modifiers::CTRL, egui::Key::Comma);
            refresh = input.consume_key(egui::Modifiers::NONE, egui::Key::F5);
            search = input.consume_key(egui::Modifiers::CTRL, egui::Key::F);
            logs = input.consume_key(egui::Modifiers::CTRL, egui::Key::L);
        });
        if logs {
            self.open_log();
            return;
        }
        if search {
            match self.page {
                Page::Content => self.content.focus_search = true,
                Page::Versions => self.version_search_focus = true,
                Page::Shots => self.gallery.focus_search = true,
                _ => {}
            }
        }
        if settings {
            self.open_settings();
        }
        if refresh {
            self.refresh();
        }
        if let Some(nav) = nav {
            self.nav_go(nav);
        }
    }

    pub(crate) fn open_settings(&mut self) {
        self.login.open = false;
        self.show_settings = true;
    }

    pub(crate) fn refresh_java(&mut self) {
        let runtimes = java::find_all(&self.root);
        self.env.java_ok = runtimes.iter().any(|runtime| runtime.major >= 21);
        self.env.java_label = runtimes
            .iter()
            .find(|runtime| runtime.major >= 21)
            .map(|runtime| runtime.label());
        self.env.javas = runtimes;
        self.play.tuning_cache.clear();
    }

    pub(crate) fn refresh(&mut self) {
        self.home.cache = None;
        self.home.verified = None;
        self.home.quick_play.clear();
        self.play.tuning_cache.clear();
        self.version_cache = None;
        let mut rows = Vec::new();
        for name in versions::names(&self.root) {
            let (mc, loader, loader_version, pack) = version_info(&self.root, &name);
            let label = match (&pack, &loader_version) {
                (Some(pack), _) => format!("{pack} · 整合包"),
                (None, Some(version)) => format!("{mc} · {} {version}", loader.name()),
                (None, None) => mc.clone(),
            };
            rows.push(Row {
                id: name.clone(),
                profile: name.clone(),
                label,
                mc,
                loader,
                loader_version,
                game_dir: versions::game_dir(&self.root, &name),
                pack,
                health: health::inspect(&self.root, &name),
            });
        }
        self.rows = rows;
        self.sort_rows();
        self.ensure_visible_selection();
    }

    /// 最近玩过的排在前面；没玩过的按整合包、游戏版本、加载器排。
    pub(crate) fn sort_rows(&mut self) {
        let stats = &self.play.stats;
        let last = |row: &Row| stats.get(&row.id).map_or(0, |stat| stat.last);
        self.rows.sort_by(|a, b| {
            last(b)
                .cmp(&last(a))
                .then_with(|| b.pack.is_some().cmp(&a.pack.is_some()))
                .then_with(|| mc_key(&b.mc).cmp(&mc_key(&a.mc)))
                .then_with(|| a.loader.rank().cmp(&b.loader.rank()))
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    pub(crate) fn selected_row(&self) -> Option<Row> {
        self.selected
            .as_ref()
            .and_then(|id| self.rows.iter().find(|r| &r.id == id))
            .cloned()
    }

    pub(crate) fn row(&self, id: &str) -> Option<Row> {
        self.rows.iter().find(|row| row.id == id).cloned()
    }

    pub(crate) fn ensure_visible_selection(&mut self) {
        let valid = self
            .selected
            .as_ref()
            .is_some_and(|id| self.rows.iter().any(|r| &r.id == id));
        if !valid {
            // 没有记住的选择时：最近玩过的那个（排在最前）；都没玩过就挑「看起来像常用版本」的：
            // 整合包、有内容、安装完整。
            let played = self
                .rows
                .first()
                .filter(|row| self.play.stats.get(&row.id).is_some_and(|s| s.last > 0));
            self.selected = played
                .or_else(|| {
                    self.rows.iter().max_by_key(|row| {
                        let content = installed::load(&row.game_dir).entries.len();
                        (row.pack.is_some(), content, row.health.missing == 0)
                    })
                })
                .map(|row| row.id.clone());
        }
    }

    /// 窗口不在前台时，在任务栏上提醒一下：装完了、游戏退出了。
    pub(crate) fn notify_if_away(&self, ctx: &egui::Context) {
        if !self.window_focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

    /// 页面最底下一条细边，右边是版本号。单独占一条，不会压住列表、卡片和按钮。
    /// 点版本号打开设置（那里能检查更新）；左边是 `Ctrl+K` 的提示。
    pub(crate) fn version_line(&mut self, ui: &mut egui::Ui) {
        let mut open = false;
        let mut palette = false;
        egui::Panel::bottom("version_line")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(18.0)
            .frame(egui::Frame::new().fill(t::bg()).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 0,
                bottom: 4,
            }))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let version = env!("CARGO_PKG_VERSION");
                    let small = |text: String| egui::RichText::new(text).size(11.0).color(t::dim());
                    let response = ui
                        .add(
                            egui::Label::new(small(format!("v{version}")))
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text(format!("Cairn {version} · 设置和检查更新"));
                    if response.clicked() {
                        open = true;
                    }
                    ui.add_space(10.0);
                    if ui
                        .add(
                            egui::Label::new(small("Ctrl+K 找任何东西".into()))
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text("版本、页面、设置和常用操作，打几个字就到")
                        .clicked()
                    {
                        palette = true;
                    }
                });
            });
        if open {
            self.open_settings();
        }
        if palette && !self.page_locked() {
            self.open_palette();
        }
    }

    /// 页面底部的一行提示：错误、完成通知，以及「查看日志」「复制」和崩溃后的「退回改动」。
    /// 所有页面共用这一处。完成通知过几秒自己消失，错误留到玩家关掉。
    pub(crate) fn message_bar(&mut self, ui: &mut egui::Ui) {
        const NOTICE_SECONDS: f64 = 6.0;
        // 安装没完成的那一页自己把原因写在正中间，不用再说一遍。
        if self.page == Page::Install && self.install.repair_target.is_some() {
            return;
        }
        let now = ui.input(|i| i.time);
        let (text, color, is_error) = match (&self.error, &self.notice) {
            (Some(error), _) => (error.clone(), t::danger(), true),
            (None, Some(notice)) => (notice.clone(), t::ok(), false),
            (None, None) => {
                self.notice_seen = None;
                return;
            }
        };
        if !is_error {
            let fresh = self
                .notice_seen
                .as_ref()
                .is_none_or(|(seen, _)| seen != &text);
            if fresh {
                self.notice_seen = Some((text.clone(), now));
            }
        }
        let mut close = false;
        let mut show_log = false;
        let mut copy = false;
        let mut recover = false;
        let has_recovery = is_error && self.play.recovery.is_some();
        let response = egui::Panel::bottom("message_bar")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(40.0)
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .inner_margin(egui::Margin::symmetric(24, 0)),
            )
            .show(ui, |ui| {
                let bar = ui.max_rect().expand2(egui::vec2(24.0, 0.0));
                ui.painter().line_segment(
                    [bar.left_top(), bar.right_top()],
                    egui::Stroke::new(1.0, t::hairline()),
                );
                ui.horizontal_centered(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("×").size(15.0)).frame(false),
                            )
                            .on_hover_text("关闭提示")
                            .clicked()
                        {
                            close = true;
                        }
                        if is_error {
                            if action_link(ui, "查看日志")
                                .on_hover_text("Ctrl+L")
                                .clicked()
                            {
                                show_log = true;
                            }
                            if action_link(ui, "复制").clicked() {
                                copy = true;
                            }
                        }
                        if has_recovery && small_accent(ui, "退回改动").clicked() {
                            recover = true;
                        }
                        // 剩下的地方给文字，放不下从末尾省略，悬停看全文。
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&text).size(13.0).color(color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&text);
                        });
                    });
                });
            })
            .response;
        if !is_error && let Some((_, since)) = &self.notice_seen {
            let left = NOTICE_SECONDS - (now - since);
            if response.contains_pointer() {
                // 指着它就不走，挪开再重新数。
                self.notice_seen = Some((text.clone(), now));
            } else if left <= 0.0 {
                close = true;
            } else {
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(left.max(0.05)));
            }
        }
        if copy {
            ui.ctx().copy_text(text);
        }
        if close {
            self.error = None;
            self.notice = None;
            self.notice_seen = None;
        }
        if show_log {
            self.open_log();
        }
        if recover {
            self.play.recovery_open = true;
        }
    }
}

pub(crate) struct App {
    // ---- 数据和选择 ----
    /// 数据目录。
    pub(crate) root: Root,
    /// 界面的上下文：后台线程做完事用它叫醒界面。
    pub(crate) ctx: egui::Context,
    /// 版本列表（原版、加载器和整合包），最近玩过的在前。
    pub(crate) rows: Vec<Row>,
    /// 当前选中的版本行；会记进 `ui.json`，下次打开还是它。
    pub(crate) selected: Option<String>,
    /// 全部账户和正在用的那个，存在数据目录的 `accounts.json`。
    pub(crate) accounts: nailong_core::accounts::Accounts,
    /// 全局启动参数、外观、窗口大小这些，存在数据目录的 `ui.json`。
    pub(crate) prefs: UiPrefs,
    // ---- 导航：哪个页面在前面 ----
    pub(crate) page: Page,
    /// 截图页的状态和全部截图（启动台那一排也用它）。
    pub(crate) gallery: crate::pages::gallery::GalleryState,
    /// 等待确认删除的版本。
    pub(crate) delete_confirm: Option<String>,
    /// 正在改名的版本和输入框里的新名字。
    pub(crate) renaming: Option<(String, String)>,
    /// 移不到回收站的版本：问玩家要不要直接删。
    pub(crate) delete_failed: Option<String>,
    /// 版本页的搜索框，和「下一帧把焦点给它」。
    pub(crate) version_query: String,
    pub(crate) version_search_focus: bool,
    /// 有任务时是否整页显示进度。离开（点侧栏）后任务在后台继续，点侧栏的进度卡回来。
    pub(crate) progress_open: bool,
    /// 上一帧的任务，用来发现「刚开始了一个任务」和「任务刚做完」。
    pub(crate) progress_seen: Option<Task>,
    /// 设置里的「清理不用的文件」。
    pub(crate) cleanup: crate::dialogs::settings::CleanupState,
    /// 当前视图的编号，变了就放一次切页动效。
    pub(crate) page_key: u8,
    /// 进入当前视图的时间。
    pub(crate) page_enter_at: f64,
    /// 窗口上一帧是否在前台：回到窗口时重新核对内容目录。
    pub(crate) window_focused: bool,
    // ---- 各页面和弹窗自己的状态 ----
    /// 启动台：摘要缓存、后台核对和截图墙。
    pub(crate) home: HomeState,
    /// 安装新版本的表单和版本目录。
    pub(crate) install: InstallForm,
    /// 内容页：发现和已安装两种视图共用的状态。
    pub(crate) content: ContentState,
    /// 存档页：世界、快照和自动备份。
    pub(crate) worlds: WorldsState,
    /// 账户弹窗和正在进行的微软登录。
    pub(crate) login: LoginState,
    /// 恢复点弹窗。
    pub(crate) rollback: RollbackState,
    /// 导出整合包的弹窗。
    pub(crate) export: crate::dialogs::export::ExportState,
    /// 换数据目录的弹窗。
    pub(crate) data_dir: crate::dialogs::datadir::DataDirState,
    /// 启动器自己的更新：检查结果、更新弹窗和下载进度。
    pub(crate) update: UpdateState,
    /// 等待确认的安装。
    pub(crate) pending_install: Option<PendingInstall>,
    /// 版本页摘要（Mod、资源包、光影、存档数）的缓存。
    pub(crate) version_cache: Option<(String, (usize, usize, usize, usize))>,
    /// 设置弹窗。
    pub(crate) show_settings: bool,
    /// 设置弹窗现在看的是哪一栏。
    pub(crate) settings_tab: crate::dialogs::settings::SettingsTab,
    /// 设置里的「查看计算」展开了没有。
    pub(crate) show_tuning: bool,
    /// 实测调参的确认弹窗。
    pub(crate) bench_confirm: bool,
    /// 启动参数窗口。
    pub(crate) launch_edit: Option<LaunchEdit>,
    /// 日志窗口；None 时关着。
    pub(crate) log_view: Option<log::LogView>,
    /// `Ctrl+K` 的命令面板；None 时关着。
    pub(crate) palette: Option<crate::dialogs::palette::Palette>,
    /// 「复制版本」的弹窗。
    pub(crate) duplicate: Option<crate::dialogs::duplicate::Duplicate>,
    // ---- 开始游戏和这台电脑 ----
    /// 开始游戏：每个版本的启动参数、算好的调参、游玩记录，以及正在运行的游戏。
    pub(crate) play: PlayState,
    /// 这台电脑：找到的 Java、配置，以及「测一下这台电脑」的结论。
    pub(crate) env: Env,
    // ---- 后台任务和提示 ----
    /// 正在跑的后台任务；同一时间只有一个。游戏运行不算（见 `play.sessions`）。
    pub(crate) busy: Option<Task>,
    /// 任务的「取消」已经按过。
    pub(crate) cancel_requested: bool,
    /// 正在跑的任务给人看的样子：步骤、进度、刚下载的文件、速度。
    pub(crate) activity: Option<Activity>,
    /// 后台任务的结果和进度从这里回来。
    pub(crate) rx: Option<mpsc::Receiver<Msg>>,
    /// 页面底部的错误，留到玩家关掉。
    pub(crate) error: Option<String>,
    /// 页面底部的提示，过几秒自己消失。
    pub(crate) notice: Option<String>,
    /// 正在显示的提示和它第一次出现的时间。
    pub(crate) notice_seen: Option<(String, f64)>,
    /// 启动器自己的记录，日志窗口的「启动器」标签显示它。
    pub(crate) log: Vec<String>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        t::sync(&ctx);
        self.poll();
        self.poll_sessions(&ctx);
        self.poll_accounts(&ctx);
        self.tick_shots(&ctx);
        self.poll_update(&ctx);
        self.poll_duplicate();
        motion::set_reduced(self.prefs.reduce_motion);
        self.handle_shortcuts(&ctx);
        self.drain_icons(&ctx);
        self.track_window(&ctx);
        if self.worlds.backup.enabled {
            ctx.request_repaint_after(Duration::from_secs(60));
            self.maybe_auto_backup();
        }
        let dropped: Vec<PathBuf> = ctx
            .input_mut(|i| std::mem::take(&mut i.raw.dropped_files))
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .collect();
        if !dropped.is_empty() {
            self.content.drop_preview = None;
            self.handle_drop(&dropped);
        }
        // 回到窗口时，文件管理器里做的改动直接反映出来，不用点「重新扫描」。
        let focused = ctx.input(|i| i.focused);
        if focused && !self.window_focused {
            self.rescan_if_changed();
            self.gallery.stale = true;
            if self.page == Page::Worlds {
                self.reload_saves();
            }
        }
        self.window_focused = focused;
        if self.busy.is_some()
            || self.login.busy
            || self.worlds.rx.is_some()
            || self.content.rx.is_some()
            || self.content.scan_rx.is_some()
            || self.rollback.rx.is_some()
            || self.content.browse_loading
            || !self.content.icons_pending.is_empty()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // 刚开始一个任务：安装、下载 Java 先整页显示进度；内容页装 Mod 留在原页，只在侧栏显示。
        if self.busy != self.progress_seen {
            match self.busy {
                Some(task) if task.shows_progress() => self.progress_open = task != Task::Mods,
                Some(_) => {}
                None => self.notify_if_away(&ctx),
            }
            self.progress_seen = self.busy;
        }
        let now = ctx.input(|input| input.time);
        let view = self.view_key();
        if view != self.page_key {
            self.page_key = view;
            // 别的页面可能改了内容或快照，回到启动台时重新读取。
            self.home.cache = None;
            self.home.verified = None;
            self.play.tuning_cache.clear();
            self.page_enter_at = now;
        }
        let enter = if self.prefs.reduce_motion {
            1.0
        } else {
            (((now - self.page_enter_at) / 0.15) as f32).clamp(0.0, 1.0)
        };
        let eased = motion::ease_out_cubic(enter);
        if enter < 1.0 {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("NAILONG_DEBUG_HOVER_CYCLE").is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        self.sidebar(ui);
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(t::bg()))
            .show(ui, |ui| {
                self.version_line(ui);
                self.message_bar(ui);
                ui.add_space((1.0 - eased) * 6.0);
                if self.progress_showing() {
                    self.progress_view(ui);
                } else {
                    match self.page {
                        Page::Home => self.main_view(ui),
                        Page::Versions => self.manage_view(ui),
                        Page::Install => self.install_view(ui),
                        Page::Content => self.mods_view(ui),
                        Page::Worlds => self.saves_view(ui),
                        Page::Shots => self.gallery_page(ui),
                    }
                }
                self.drop_overlay(ui);
                // 用一层底色做淡入：嵌套面板各有画笔，逐个调透明度并不可靠。
                let veil = 1.0 - eased;
                if veil > 0.001 {
                    ui.painter()
                        .rect_filled(ui.max_rect(), 0.0, t::tint(t::bg(), veil));
                }
            });
        self.account_overlay(&ctx);
        self.settings_overlay(&ctx);
        self.bench_overlay(&ctx);
        self.rollback_overlay(&ctx);
        self.install_review_overlay(&ctx);
        self.log_overlay(&ctx);
        self.launch_overlay(&ctx);
        self.update_overlay(&ctx);
        self.export_overlay(&ctx);
        self.data_dir_overlay(&ctx);
        self.duplicate_overlay(&ctx);
        self.recovery_overlay(&ctx);
        if self.gallery.shots.as_ref().is_some_and(|s| s.viewing()) {
            let faces = self.player_faces(&ctx);
            if let Some(shots) = &mut self.gallery.shots {
                shots.viewer(&ctx, &faces);
            }
        }
        self.palette_overlay(&ctx);
        self.remember_selection();
        motion::focus_ring(&ctx);
        motion::pointer_on_clickables(&ctx);
    }

    /// 关掉 Cairn 时：记下窗口大小；把下载好的新版本换上（Windows），下次打开就是新版本。
    /// 还在运行的游戏不受影响，照常玩。
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_ui_config();
        let _ = nailong_core::update::apply_staged(&self.root.cache(), false);
    }
}

impl App {
    /// 记下窗口大小和缩放（关掉时写进 `ui.json`）。最大化时不记大小，还原后还是原来那么大。
    fn track_window(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let (size, maximized) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport.inner_rect.map(|rect| rect.size()),
                viewport.maximized.unwrap_or(false),
            )
        });
        self.prefs.maximized = maximized;
        self.prefs.zoom = zoom;
        if !maximized
            && let Some(size) = size
            && size.x > 100.0
            && size.y > 100.0
        {
            // 存没缩放时的大小：下次打开先按它建窗口，再套上缩放。
            self.prefs.window = Some([(size.x * zoom).round(), (size.y * zoom).round()]);
        }
    }
}
