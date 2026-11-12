//! 开始游戏：合并全局和这个版本的启动参数、选 Java、算 JVM 参数，然后在后台启动。

use crate::prelude::*;

impl App {
    /// 这个版本单独的启动参数；没设置过时全部跟随全局。
    pub(crate) fn version_cfg(&self, row_id: &str) -> VersionLaunch {
        self.play.versions.get(row_id).cloned().unwrap_or_default()
    }

    /// 全局设置叠上这个版本自己的设置。
    pub(crate) fn tuning_settings(&self, cfg: &VersionLaunch) -> tuning::Settings {
        tuning::Settings {
            memory_mb: cfg.memory.map_or(self.prefs.memory, MemoryPick::memory_mb),
            min_memory_mb: None,
            gc: cfg.gc.unwrap_or(self.prefs.gc),
            extra: tuning::split_args(cfg.jvm_args.as_deref().unwrap_or(&self.prefs.jvm_args)),
            ..Default::default()
        }
    }

    /// 指定路径的 Java：先在检测到的里找，找不到再读一次它的版本。
    pub(crate) fn java_at(&mut self, path: &std::path::Path) -> Option<java::JavaRuntime> {
        let same = |runtime: &java::JavaRuntime| {
            runtime.path == path
                || std::fs::canonicalize(&runtime.path).ok() == std::fs::canonicalize(path).ok()
        };
        if let Some(found) = self
            .env
            .javas
            .iter()
            .chain(&self.env.extra_javas)
            .find(|r| same(r))
        {
            return Some(found.clone());
        }
        let runtime = java::probe(path).ok()?;
        self.env.extra_javas.push(runtime.clone());
        Some(runtime)
    }

    /// 这个版本启动时会用的堆和 GC 参数，和 `launch` 的算法相同；只在缓存失效时读盘。
    pub(crate) fn tuning_for(&mut self, row: &Row) -> Option<tuning::Plan> {
        let cfg = self.version_cfg(&row.id);
        self.plan_for(row, &cfg).map(|(plan, _)| plan)
    }

    /// 缓存按「版本 + 指定的 Java」区分：启动参数窗口里换 Java 时不会串。
    pub(crate) fn tuning_key(row_id: &str, cfg: &VersionLaunch) -> String {
        format!("{row_id}\u{1f}{:?}", cfg.java)
    }

    /// 按给定的启动参数（可以是还没保存的草稿）算出会用的 JVM 参数和 Java。
    pub(crate) fn plan_for(
        &mut self,
        row: &Row,
        cfg: &VersionLaunch,
    ) -> Option<(tuning::Plan, java::JavaRuntime)> {
        let key = Self::tuning_key(&row.id, cfg);
        if !self.play.tuning_cache.contains_key(&key) {
            let game_dir = row.game_dir.clone();
            let chosen = cfg.java.as_deref().and_then(|path| self.java_at(path));
            let entry = if cfg.java.is_some() && chosen.is_none() {
                None
            } else {
                tuning::workload(
                    &self.root,
                    &row.profile,
                    &game_dir,
                    &self.env.javas,
                    chosen.as_ref(),
                )
                .ok()
                .map(|(work, java)| {
                    let learned = learned::load(&self.root, &row.profile, &game_dir);
                    (work, java, learned)
                })
            };
            self.env.machine = tuning::Machine::detect();
            self.play.tuning_cache.insert(key.clone(), entry);
        }
        let (work, java, learned) = self.play.tuning_cache.get(&key)?.as_ref()?;
        let settings = tuning::Settings {
            bench: self
                .env
                .bench
                .as_ref()
                .and_then(|store| store.verdict(java)),
            learned: learned.clone(),
            ..self.tuning_settings(cfg)
        };
        Some((
            tuning::plan(&self.env.machine, work, &settings),
            java.clone(),
        ))
    }

    /// 缓存里这个版本当前设置对应的那一条。
    pub(crate) fn tuning_entry(&self, row_id: &str) -> Option<&TuningEntry> {
        let key = Self::tuning_key(row_id, &self.version_cfg(row_id));
        self.play.tuning_cache.get(&key)?.as_ref()
    }

    /// 例如「内存 5 GB（自动）」。
    pub(crate) fn memory_label(&mut self, row: &Row) -> String {
        match self.tuning_for(row) {
            Some(plan) if plan.auto_memory => format!("内存 {}（自动）", tuning::gb(plan.heap_mb)),
            Some(plan) => format!("内存 {}", tuning::gb(plan.heap_mb)),
            None => match self.prefs.memory {
                Some(mb) => format!("内存 {}", tuning::gb(mb)),
                None => "内存自动".into(),
            },
        }
    }

    /// 这个版本的游戏在不在运行。
    pub(crate) fn is_running(&self, row_id: &str) -> bool {
        self.play.sessions.iter().any(|s| s.row_id == row_id)
    }

    /// 有没有游戏在运行（哪个版本都算）。
    pub(crate) fn any_game(&self) -> bool {
        !self.play.sessions.is_empty()
    }

    pub(crate) fn session(&self, row_id: &str) -> Option<&Session> {
        self.play.sessions.iter().find(|s| s.row_id == row_id)
    }

    /// 现在不能启动这个版本的原因；`None` 表示可以。游戏运行不占任务，
    /// 所以装别的版本、下别的版本的 Mod 时照样能玩。
    pub(crate) fn launch_block(&self, row: &Row) -> Option<String> {
        if self.is_running(&row.id) {
            return Some("这个版本已经在运行".into());
        }
        let touching = match self.busy {
            Some(Task::Bench) => true,
            Some(Task::Files) => true,
            Some(Task::Install) => match &self.install.repair_target {
                Some(RepairTarget::Loader { name, .. }) => name == &row.id,
                _ => false,
            },
            Some(Task::Mods) => self
                .content
                .row
                .as_ref()
                .is_some_and(|target| target.id == row.id),
            Some(Task::Java) | Some(Task::Export) | None => false,
        };
        touching.then(|| self.busy_reason().to_string())
    }

    /// 启动当前选中的版本，停在主菜单。
    pub(crate) fn start_launch(&mut self) {
        if let Some(row) = self.selected_row() {
            self.launch_row(&row, None);
        }
    }

    /// 启动一个版本；`quick` 不为空时直接进那个世界或服务器。
    pub(crate) fn launch_row(&mut self, row: &Row, quick: Option<launch::QuickPlay>) {
        if let Some(reason) = self.launch_block(row) {
            self.error = Some(reason);
            return;
        }
        if row.health.missing > 0 {
            self.error = Some("这个版本缺少文件，先补全再启动".into());
            return;
        }
        let cfg = self.version_cfg(&row.id);
        let required = launch::required_java_for(&self.root, &row.profile).unwrap_or(8);
        match &cfg.java {
            Some(path) => match self.java_at(path) {
                Some(java) if java.major < required => {
                    self.error = Some(format!(
                        "这个版本需要 Java {required}，启动参数里指定的是 Java {}；在启动参数里换一个",
                        java.major
                    ));
                    return;
                }
                Some(_) => {}
                None => {
                    self.error = Some(format!(
                        "启动参数里指定的 Java 不见了：{}；在启动参数里换一个",
                        path.display()
                    ));
                    return;
                }
            },
            None => {
                if !self.env.javas.iter().any(|java| java.major >= required) {
                    self.refresh_java();
                }
                if !self.env.javas.iter().any(|java| java.major >= required) {
                    if self.busy.is_some() {
                        self.error = Some(format!(
                            "这个版本需要 Java {required}，现在没有；{}",
                            self.busy_reason()
                        ));
                        return;
                    }
                    // 没有合适的 Java：直接下载官方的，下完接着启动。玩家不用自己去找。
                    let component = launch::java_requirement(&self.root, &row.profile)
                        .ok()
                        .and_then(|(_, component)| component);
                    self.start_java_download(required, component, Some((row.id.clone(), quick)));
                    return;
                }
            }
        }
        self.error = None;
        self.notice = None;
        self.play.recovery = None;
        self.selected = Some(row.id.clone());
        self.remember_selection();
        let target = match &quick {
            Some(launch::QuickPlay::World(name)) => Some(name.clone()),
            Some(launch::QuickPlay::Server(address)) => Some(address.clone()),
            None => None,
        };
        self.log.push(match &target {
            Some(target) => format!("启动 {} → {target}", row.id),
            None => format!("启动 {}", row.id),
        });
        let log_path = launch::log_path(&self.root, &row.profile);
        self.play.last = Some(log::LastLaunch {
            label: row.id.clone(),
            log: log_path.clone(),
            game_dir: row.game_dir.clone(),
            started: std::time::SystemTime::now(),
            outcome: log::Outcome::Running,
        });
        self.play
            .tuning_cache
            .retain(|key, _| !key.starts_with(&format!("{}\u{1f}", row.id)));
        if let Some(plan) = self.tuning_for(row) {
            self.log
                .push(format!("JVM {}，{}", plan.headline(), plan.basis));
            self.log
                .extend(plan.warnings.iter().map(|w| format!("注意：{w}")));
        }
        let root = self.root.clone();
        let tuning = self.tuning_settings(&cfg);
        let [width, height] = cfg.window();
        let java_path = cfg.java.clone();
        let account_id = self.accounts.current.clone();
        let fallback_name = self.prefs.username.clone();
        // 截图记下的「地点：版本」用版本名。
        let version_label = row.id.clone();
        let embed_shots = self.prefs.embed_shots;
        let (tx, rx) = mpsc::channel();
        self.play.sessions.push(Session {
            row_id: row.id.clone(),
            started: Instant::now(),
            started_at: unix_now(),
            handle: None,
            rx,
            target,
            log: log_path,
            game_dir: row.game_dir.clone(),
            window_seen: false,
        });
        let row = row.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            use nailong_core::accounts::{self, Ready};
            let send = |msg: SessionMsg| {
                let _ = tx.send(msg);
                ctx.request_repaint();
            };
            let identity = match &account_id {
                Some(id) => match accounts::ready(&root, id) {
                    Ok(Ready::Go(identity)) => identity,
                    Ok(Ready::Relogin { id, reason }) => {
                        send(SessionMsg::NeedRelogin(id, reason));
                        return;
                    }
                    Err(e) => {
                        send(SessionMsg::Failed(e.to_string()));
                        return;
                    }
                },
                None => accounts::Identity {
                    uuid: launch::offline_uuid(&fallback_name),
                    name: fallback_name,
                    access_token: None,
                    xuid: None,
                    client_id: None,
                    user_type: "legacy",
                    external: None,
                    kind: "离线".into(),
                },
            };
            let injector = match &identity.external {
                Some(external) => match nailong_core::yggdrasil::injector_jar(&root) {
                    Ok(jar) => Some(launch::Injector {
                        jar,
                        api: external.api.clone(),
                        metadata: external.metadata.clone(),
                    }),
                    Err(e) => {
                        send(SessionMsg::Failed(format!(
                            "authlib-injector 下载失败：{e}"
                        )));
                        return;
                    }
                },
                None => None,
            };
            let opts = LaunchOptions {
                username: identity.name.clone(),
                uuid: Some(identity.uuid.clone()),
                access_token: identity.access_token.clone(),
                xuid: identity.xuid.clone(),
                client_id: identity.client_id.clone(),
                user_type: Some(identity.user_type.to_string()),
                injector,
                shots: Some(nailong_core::shots::Tag {
                    version: version_label,
                    player: identity.name.clone(),
                    player_uuid: identity.uuid.clone(),
                    account: identity.kind.clone(),
                    embed: embed_shots,
                }),
                memory_mb: tuning.memory_mb,
                gc: tuning.gc,
                jvm_args: tuning.extra,
                game_dir: Some(row.game_dir.clone()),
                java: java_path,
                width,
                height,
                quick_play: quick,
                ..Default::default()
            };
            let game = match launch::start(&root, &row.profile, &opts) {
                Ok(game) => game,
                Err(e) => {
                    send(SessionMsg::Failed(e.to_string()));
                    return;
                }
            };
            let handle = game.handle();
            send(SessionMsg::Started(handle.clone()));
            // 窗口出来时叫醒界面一次，状态从「正在启动」变成「运行中」。
            {
                let (handle, ctx) = (handle.clone(), ctx.clone());
                std::thread::spawn(move || {
                    for _ in 0..600 {
                        if handle.window_shown() || handle.stopping() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    ctx.request_repaint();
                });
            }
            let msg = match game.wait() {
                Ok(0) => SessionMsg::Exited(0, None),
                Ok(code) if handle.stopping() => SessionMsg::Exited(code, None),
                Ok(code) => {
                    // 在后台线程里读日志找原因，界面不用等。
                    let reason = std::fs::read(launch::log_path(&root, &row.profile))
                        .ok()
                        .and_then(|bytes| {
                            let tail = &bytes[bytes.len().saturating_sub(2 << 20)..];
                            launch::diagnose(&String::from_utf8_lossy(tail))
                        });
                    SessionMsg::Exited(code, reason)
                }
                Err(e) => SessionMsg::Failed(e.to_string()),
            };
            send(msg);
        });
    }

    /// 结束一个正在运行的游戏。
    pub(crate) fn stop_game(&mut self, row_id: &str) {
        if let Some(session) = self.play.sessions.iter().find(|s| s.row_id == row_id) {
            match &session.handle {
                Some(handle) => {
                    handle.stop();
                    self.log.push(format!("结束 {row_id}"));
                }
                // 进程还没起来（在刷新凭证、解压文件）：等它起来再结束。
                None => self.play.stop_pending.push(row_id.to_string()),
            }
        }
        self.play.stop_confirm = None;
    }

    /// 每帧收游戏那边的消息：进程起来了、窗口出来了、退出了。
    pub(crate) fn poll_sessions(&mut self, ctx: &egui::Context) {
        if self.play.sessions.is_empty() {
            return;
        }
        let mut events: Vec<(usize, SessionMsg)> = Vec::new();
        let mut window_shown = false;
        let mut starting = false;
        for (index, session) in self.play.sessions.iter_mut().enumerate() {
            loop {
                match session.rx.try_recv() {
                    Ok(msg) => events.push((index, msg)),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // 线程没留下结果就没了：当成没启动起来，别让这一局永远挂着。
                        if !events.iter().any(|(i, m)| *i == index && m.is_final()) {
                            events.push((index, SessionMsg::Failed("启动中断了".into())));
                        }
                        break;
                    }
                }
            }
            if let Some(handle) = &session.handle {
                if handle.window_shown() && !session.window_seen {
                    session.window_seen = true;
                    window_shown = true;
                }
                starting |= !session.window_seen;
            } else {
                starting = true;
            }
        }
        if window_shown && self.prefs.minimize_on_launch && !self.play.minimized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.play.minimized = true;
        }
        let mut finished: Vec<usize> = Vec::new();
        for (index, msg) in events {
            let (row_id, started, started_at, log_path, game_dir, stopping) = {
                let session = &self.play.sessions[index];
                (
                    session.row_id.clone(),
                    session.started,
                    session.started_at,
                    session.log.clone(),
                    session.game_dir.clone(),
                    session.handle.as_ref().is_some_and(|h| h.stopping()),
                )
            };
            let set_outcome = |app: &mut Self, outcome: log::Outcome| {
                if let Some(last) = &mut app.play.last
                    && last.log == log_path
                {
                    last.outcome = outcome;
                }
            };
            match msg {
                SessionMsg::Started(handle) => {
                    if let Some(at) = self.play.stop_pending.iter().position(|id| id == &row_id) {
                        self.play.stop_pending.remove(at);
                        handle.stop();
                    }
                    self.play.sessions[index].handle = Some(handle);
                }
                SessionMsg::Exited(code, reason) => {
                    finished.push(index);
                    set_outcome(self, log::Outcome::Exited(code));
                    let seconds = started.elapsed().as_secs();
                    // 被玩家结束的不算崩溃，也不算正常玩完的一局。
                    let recorded = if stopping && code != 0 { 1 } else { code };
                    let stat = nailong_core::played::record(
                        &self.root, &row_id, started_at, seconds, recorded,
                    )
                    .ok();
                    if let Some(stat) = &stat {
                        self.play.stats.insert(row_id.clone(), stat.clone());
                    }
                    // 启动前可能刷新过凭证，重新读一遍；这局截的图也该出现在截图页了。
                    self.accounts = nailong_core::accounts::Accounts::load(&self.root);
                    self.gallery.stale = true;
                    self.home.cache = None;
                    self.home.verified = None;
                    self.play.tuning_cache.clear();
                    if stopping {
                        self.notice = Some(format!("已结束「{row_id}」"));
                    } else if code == 0 {
                        self.notice = Some(format!(
                            "「{row_id}」已退出 · 玩了 {}",
                            nailong_core::played::human(seconds)
                        ));
                    } else {
                        self.log.push(format!("{row_id} 异常退出（代码 {code}）"));
                        self.error = Some(match reason {
                            Some(reason) => format!("游戏异常退出（代码 {code}）：{reason}"),
                            None => {
                                format!("游戏异常退出（代码 {code}），日志里标红的是出错的地方")
                            }
                        });
                        // 坏了能救：上次正常游戏之后改过内容的话，给一条退回去的路。
                        self.play.recovery = stat
                            .as_ref()
                            .and_then(|stat| Recovery::find(&row_id, &game_dir, stat.last_good));
                    }
                }
                SessionMsg::Failed(e) => {
                    finished.push(index);
                    set_outcome(self, log::Outcome::Failed(e.clone()));
                    self.accounts = nailong_core::accounts::Accounts::load(&self.root);
                    self.error = Some(format!("启动失败：{e}"));
                    self.log.push(format!("启动失败: {e}"));
                }
                SessionMsg::NeedRelogin(id, reason) => {
                    finished.push(index);
                    set_outcome(self, log::Outcome::Failed(reason.clone()));
                    self.accounts = nailong_core::accounts::Accounts::load(&self.root);
                    self.log.push(format!("账户要重新登录：{reason}"));
                    self.relogin(&id, Some(reason));
                }
            }
        }
        if !finished.is_empty() {
            finished.sort_unstable();
            finished.dedup();
            for index in finished.into_iter().rev() {
                self.play.sessions.remove(index);
            }
            self.sort_rows();
            if self.play.sessions.is_empty() {
                self.play.stop_pending.clear();
                // 是我们收起来的窗口，游戏都退出了就放回来。
                if self.play.minimized {
                    self.play.minimized = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                // 每玩完一局就给世界留一份快照，比按时间间隔更贴近“坏了能救”。
                if self.worlds.backup.enabled && !self.worlds.busy {
                    self.start_auto_backup();
                }
            }
            self.notify_if_away(ctx);
        }
        if starting {
            ctx.request_repaint_after(Duration::from_millis(500));
        } else if !self.play.sessions.is_empty() && self.window_focused {
            // 只为了「已运行 12 分钟」这个数字；窗口在后台时不重绘。
            ctx.request_repaint_after(Duration::from_secs(20));
        }
    }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// 一局正在进行的游戏。
pub(crate) struct Session {
    /// 版本行的 id（版本名）。
    pub(crate) row_id: String,
    pub(crate) started: Instant,
    /// 开始的时刻（Unix 秒），记游玩时长用。
    pub(crate) started_at: u64,
    /// 进程起来以后才有；之前在准备（刷新凭证、解压文件）。
    pub(crate) handle: Option<launch::Handle>,
    rx: mpsc::Receiver<SessionMsg>,
    /// 直接进的世界或服务器。
    pub(crate) target: Option<String>,
    /// 这一局的游戏输出。
    pub(crate) log: PathBuf,
    pub(crate) game_dir: PathBuf,
    /// 已经看到过窗口出来。
    window_seen: bool,
}

/// 一局游戏走到哪了。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    /// 刷新凭证、解压文件，进程还没起来。
    Preparing,
    /// 进程起来了，窗口还没出来。
    Starting,
    /// 窗口出来了。
    Running,
    /// 按过结束，等它退出。
    Stopping,
}

impl Session {
    /// 截图和冒烟测试用的假会话：看起来在运行，没有真的进程。
    #[cfg(debug_assertions)]
    pub(crate) fn debug(row: &Row) -> Self {
        let (tx, rx) = mpsc::channel();
        // 发送端一直留着，这一局就不会被当成「启动中断」。
        std::mem::forget(tx);
        Self {
            row_id: row.id.clone(),
            started: Instant::now() - Duration::from_secs(13 * 60),
            started_at: unix_now() - 13 * 60,
            handle: None,
            rx,
            target: None,
            log: PathBuf::new(),
            game_dir: row.game_dir.clone(),
            window_seen: true,
        }
    }

    pub(crate) fn phase(&self) -> Phase {
        match &self.handle {
            #[cfg(debug_assertions)]
            None if self.window_seen => Phase::Running,
            None => Phase::Preparing,
            Some(handle) if handle.stopping() => Phase::Stopping,
            // 认不出窗口的版本（输出格式特别）过一阵也当它起来了。
            Some(handle) if handle.window_shown() || self.started.elapsed().as_secs() > 120 => {
                Phase::Running
            }
            Some(_) => Phase::Starting,
        }
    }

    /// 「正在启动」「运行中 · 12 分钟」这样的一句。
    pub(crate) fn status(&self) -> String {
        match self.phase() {
            Phase::Preparing => "正在准备".into(),
            Phase::Starting => "正在启动".into(),
            Phase::Stopping => "正在结束".into(),
            Phase::Running => {
                let minutes = self.started.elapsed().as_secs() / 60;
                if minutes == 0 {
                    "运行中".into()
                } else {
                    format!("运行中 · {}", nailong_core::played::human(minutes * 60))
                }
            }
        }
    }
}

enum SessionMsg {
    Started(launch::Handle),
    /// 退出码，以及从游戏输出里认出的原因。
    Exited(i32, Option<String>),
    Failed(String),
    /// 开始游戏前发现账户凭证失效：账户编号和原因。
    NeedRelogin(String, String),
}

impl SessionMsg {
    fn is_final(&self) -> bool {
        !matches!(self, Self::Started(_))
    }
}

/// 游戏异常退出后的一条退路：上次正常游戏之后改过内容，可以退回到改之前。
#[derive(Clone)]
pub(crate) struct Recovery {
    pub(crate) row_id: String,
    pub(crate) game_dir: PathBuf,
    /// 要退回到的恢复点：上次正常游戏之后最早的那个。
    pub(crate) point: rollback::Snapshot,
    /// 那之后一共改过几次。
    pub(crate) changes: usize,
    /// 上次正常游戏是什么时候（Unix 秒）。
    pub(crate) last_good: u64,
}

impl Recovery {
    /// 上次正常游戏（`last_good`）之后有没有改过内容；没正常玩过或没改过就没有退路可给。
    pub(crate) fn find(row_id: &str, game_dir: &std::path::Path, last_good: u64) -> Option<Self> {
        if last_good == 0 {
            return None;
        }
        let after: Vec<rollback::Snapshot> = rollback::list(game_dir)
            .ok()?
            .into_iter()
            .filter(|point| point.created_at > last_good && !is_safety_point(&point.reason))
            .collect();
        let point = after.iter().min_by_key(|point| point.created_at)?.clone();
        Some(Self {
            row_id: row_id.to_string(),
            game_dir: game_dir.to_path_buf(),
            point,
            changes: after.len(),
            last_good,
        })
    }
}

/// 恢复前自动留的那一份不算「改过内容」。
fn is_safety_point(reason: &str) -> bool {
    reason.contains("恢复前")
}

/// 开始游戏：每个版本的启动参数、算好的调参，以及正在运行的游戏。
pub(crate) struct PlayState {
    /// 每个版本的负载（Java、加载器、Mod 数）和实测记录（以及它是否仍然有效），
    /// 进页面和内容变化时重算。
    pub(crate) tuning_cache: HashMap<String, Option<TuningEntry>>,
    /// 每个版本单独的启动参数，键是版本行的 id。
    pub(crate) versions: BTreeMap<String, VersionLaunch>,
    /// 最近一次启动的日志在哪、结果如何。
    pub(crate) last: Option<log::LastLaunch>,
    /// 正在运行的游戏。可以同时开几个不同的版本；运行中的版本内容和存档不能改。
    pub(crate) sessions: Vec<Session>,
    /// 进程还没起来就按了结束的版本：起来以后马上结束。
    pub(crate) stop_pending: Vec<String>,
    /// 等待确认「结束游戏」的版本。
    pub(crate) stop_confirm: Option<String>,
    /// 每个版本玩了多久、上次什么时候玩的。
    pub(crate) stats: nailong_core::played::Played,
    /// 游戏异常退出后可以退回的恢复点。
    pub(crate) recovery: Option<Recovery>,
    /// 「退回改动」的确认窗口开着。
    pub(crate) recovery_open: bool,
    /// 窗口是游戏启动时我们收起来的。
    pub(crate) minimized: bool,
    /// Java 下完后接着启动的版本行，以及要直接进的世界。
    pub(crate) launch_after_java: Option<(String, Option<launch::QuickPlay>)>,
}

impl App {
    /// 下载官方的 Java 运行时。`then_launch` 是下完后要启动的版本行（和要直接进的世界）。
    pub(crate) fn start_java_download(
        &mut self,
        required: u32,
        component: Option<String>,
        then_launch: Option<(String, Option<launch::QuickPlay>)>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.error = None;
        self.notice = None;
        self.busy = Some(Task::Java);
        self.cancel_requested = false;
        cancel::reset();
        self.play.launch_after_java = then_launch;
        self.activity = Some(
            Activity::new(format!("Java {required} · 官方运行时"), "查找官方的 Java")
                .expect(crate::activity::plan::JAVA),
        );
        self.log.push(format!("下载 Java {required}"));
        let root = self.root.clone();
        let source = self.prefs.source.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let progress = tx.clone();
            let sink = from_fn(move |p| {
                let _ = progress.send(Msg::Progress(p));
            });
            let result = nailong_core::http::Http::new(&source).and_then(|http| {
                let offer = nailong_core::runtime::find(&http, required, component.as_deref())?;
                nailong_core::runtime::install(&root, &http, &offer, &sink)
                    .map(|_| format!("Java {}（官方运行时 {}）", offer.major, offer.version))
            });
            let msg = match result {
                Ok(label) => Msg::JavaDone(Ok(label)),
                Err(Error::Cancelled) => Msg::JavaCancelled,
                Err(e) => Msg::JavaDone(Err(e.to_string())),
            };
            let _ = tx.send(msg);
        });
    }

    /// Java 下完：刷新 Java 列表；是从「开始游戏」来的就接着启动。空的错误表示取消了。
    pub(crate) fn finish_java_download(&mut self, result: Result<String, String>) {
        self.busy = None;
        self.rx = None;
        self.activity = None;
        self.cancel_requested = false;
        cancel::reset();
        let then = self.play.launch_after_java.take();
        match result {
            Ok(label) => {
                self.refresh_java();
                self.play.tuning_cache.clear();
                self.log.push(format!("已装好 {label}"));
                self.notice = Some(format!("已装好 {label}"));
                if let Some((id, quick)) = then
                    && let Some(row) = self.rows.iter().find(|row| row.id == id).cloned()
                {
                    self.launch_row(&row, quick);
                }
            }
            Err(e) if e.is_empty() => {
                self.notice = Some("已停止下载 Java；下载好的部分会留着，下次接着下".into());
            }
            Err(e) => {
                self.log.push(format!("Java 下载失败: {e}"));
                self.error = Some(format!("Java 没下载下来：{e}"));
            }
        }
    }
}
