//! 启动器自己的更新。只在打开时安静地查一次（10 分钟内查过就不查），有新版本时侧栏出现
//! 一个小提示。打开了「自动更新」时：
//!
//! - Windows：在后台下载、校验，关掉 Cairn 时换成新版本；侧栏提示变成「重启更新」。
//! - Linux（deb + apt 源）：在后台启动 `cairn-update.service` 从 apt 源升级，不弹窗；
//!   程序被换掉后提示重启。授权不了时，点「更新」弹出系统的授权窗口，装不上就给出能复制的命令。

use crate::prelude::*;
use nailong_core::update::{self, Install, Release};

/// 打开时，离上次检查不到这么久就不查（反复重启时不用每次都问 GitHub）。
const CHECK_ON_START_AFTER_SECS: u64 = 10 * 60;

enum UpdateMsg {
    Checked(Result<Release, String>),
    Progress(Progress),
    /// Linux：下载好了，正在等系统的授权窗口。
    Authorizing,
    /// 新版本就绪，重启 Cairn 就是它。
    Ready(String),
    /// 下载好了 deb，要玩家自己装；附上没能自动装的原因。
    Manual(PathBuf, Option<String>),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct UpdateState {
    /// 比现在新的版本；没有新版本或还没查时为 None。
    pub(crate) latest: Option<Release>,
    pub(crate) checking: bool,
    /// 手动检查后给出的结果，例如「已经是最新版」。
    pub(crate) status: Option<String>,
    pub(crate) open: bool,
    /// 这次运行的 Cairn 是怎么装的。
    pub(crate) kind: Install,
    /// 新版本已经就绪，重启 Cairn 就是它：Windows 下载好了等着换上，Linux 已经装好。
    pub(crate) ready: Option<String>,
    rx: Option<mpsc::Receiver<UpdateMsg>>,
    /// 正在下载的进度。
    activity: Option<Activity>,
    /// 正在等系统的授权窗口。
    authorizing: bool,
    /// 下载好了，要玩家自己装的 deb。
    manual: Option<PathBuf>,
    error: Option<String>,
    started: bool,
    /// 上次看程序有没有被换掉的时间。
    tick: Option<Instant>,
    /// Linux：已经在后台启动了系统的更新服务。
    system: bool,
}

impl UpdateState {
    /// 侧栏的提示：版本号，和是不是已经就绪（只差重启）。
    pub(crate) fn badge(&self, skipped: &str) -> Option<(String, bool)> {
        if let Some(version) = &self.ready {
            return Some((version.clone(), true));
        }
        self.latest
            .as_ref()
            .filter(|release| release.version != skipped)
            .map(|release| (release.version.clone(), false))
    }

    fn busy(&self) -> bool {
        self.activity.is_some() || self.authorizing
    }

    /// 截图核对用：`update`、`update-apt`、`update-ready`、`update-linux-ready`、
    /// `update-manual`、`update-auth`、`update-sidebar`、`update-sidebar-new`（只显示侧栏提示）、
    /// `update-settings`、`update-settings-apt`（设置里的「自动更新」）。
    #[cfg(debug_assertions)]
    pub(crate) fn debug(&mut self, screen: &str) {
        self.started = true;
        self.tick = Some(Instant::now());
        self.latest = Some(Release {
            version: "0.42.0".into(),
            notes: "自动更新\n\n\
                    * Windows：后台下载，关掉 Cairn 时换成新版本\n\
                    * Linux：deb 带上 apt 源，系统每天自动更新"
                .into(),
            page: update::release_page("0.42.0"),
            assets: Vec::new(),
        });
        self.kind = if cfg!(windows) {
            Install::Exe
        } else {
            Install::AptRepo
        };
        self.open = !screen.starts_with("update-sidebar") && !screen.starts_with("update-settings");
        match screen {
            "update-apt" => self.kind = Install::AptRepo,
            "update-ready" | "update-sidebar" => {
                self.kind = Install::Exe;
                self.ready = Some("0.42.0".into());
            }
            "update-linux-ready" => self.ready = Some("0.42.0".into()),
            "update-manual" => {
                self.kind = Install::Deb;
                self.manual = Some(PathBuf::from("/home/player/下载/cairn_0.41.0_amd64.deb"));
                self.error = Some("没有授权".into());
            }
            "update-auth" => self.authorizing = true,
            "update-sidebar-new" => self.kind = Install::Exe,
            "update-settings" => self.kind = Install::Exe,
            "update-settings-apt" => self.kind = Install::AptRepo,
            _ => self.kind = Install::Exe,
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 更新说明是 Markdown：去掉标题井号和加粗，列表换成圆点，够看就行。
fn plain_notes(notes: &str) -> Vec<String> {
    notes
        .lines()
        .map(|line| {
            let line = line.trim_end().replace("**", "");
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix("- "))
            {
                format!("· {rest}")
            } else {
                trimmed.trim_start_matches('#').trim_start().to_string()
            }
        })
        .filter(|line| !line.contains("Co-Authored-By"))
        .collect()
}

impl App {
    /// 每帧调用：第一次时清理上次更新留下的文件、检查新版本；之后每分钟（后台在装时每
    /// 10 秒）看一次程序有没有被系统换掉；收后台结果。
    pub(crate) fn poll_update(&mut self, ctx: &egui::Context) {
        let cache = self.root.cache();
        let mut starting = false;
        if !self.update.started {
            self.update.started = true;
            starting = true;
            self.update.kind = update::install_kind();
            update::cleanup(&cache);
            // 上次关掉时没换成（比如被强制结束）：重启就换上。
            self.update.ready = update::staged(&cache).map(|staged| staged.version);
        }
        if starting
            && self.update.rx.is_none()
            && now().saturating_sub(self.prefs.update_checked) > CHECK_ON_START_AFTER_SECS
        {
            self.check_update(ctx, false);
        }
        let every = Duration::from_secs(if self.update.system { 10 } else { 60 });
        let due = self.update.tick.is_none_or(|tick| tick.elapsed() >= every);
        if due {
            self.update.tick = Some(Instant::now());
            ctx.request_repaint_after(every + Duration::from_secs(1));
            if self.update.ready.is_none()
                && matches!(self.update.kind, Install::AptRepo | Install::Deb)
                && update::replaced_on_disk()
            {
                self.update.ready = update::installed_version()
                    .filter(|version| update::is_newer(version, update::current()));
            }
        }
        let Some(rx) = &self.update.rx else {
            return;
        };
        let mut done = false;
        let mut auto = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                UpdateMsg::Checked(result) => {
                    self.update.checking = false;
                    self.prefs.update_checked = now();
                    self.save_ui_config();
                    done = true;
                    match result {
                        Ok(release) if update::is_newer(&release.version, update::current()) => {
                            self.update.status = None;
                            auto = self.prefs.auto_update
                                && release.version != self.prefs.update_skip
                                && self.update.ready.as_deref() != Some(release.version.as_str());
                            self.update.latest = Some(release);
                        }
                        Ok(_) => {
                            self.update.latest = None;
                            self.update.status =
                                Some(format!("已经是最新版 {}", update::current()));
                        }
                        Err(e) => self.update.status = Some(format!("没查到：{e}")),
                    }
                }
                UpdateMsg::Progress(p) => {
                    if let Some(activity) = &mut self.update.activity {
                        activity.update(p);
                    }
                }
                UpdateMsg::Authorizing => {
                    self.update.activity = None;
                    self.update.authorizing = true;
                }
                UpdateMsg::Ready(version) => {
                    done = true;
                    self.update.activity = None;
                    self.update.authorizing = false;
                    self.update.ready = Some(version);
                }
                UpdateMsg::Manual(path, why) => {
                    done = true;
                    self.update.activity = None;
                    self.update.authorizing = false;
                    self.update.manual = Some(path);
                    self.update.error = why;
                }
                UpdateMsg::Failed(e) => {
                    done = true;
                    self.update.activity = None;
                    self.update.authorizing = false;
                    self.update.error = Some(e);
                }
            }
        }
        if done {
            self.update.rx = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if auto {
            match self.update.kind {
                Install::Exe if update::can_replace_exe() => self.start_update(ctx),
                Install::AptRepo if !self.update.system => {
                    // 不弹窗；授权不了就留着侧栏提示，玩家点「更新」时再输密码。
                    self.update.system = update::start_system_update();
                }
                _ => {}
            }
        }
    }

    /// 在后台查 GitHub 上的最新版本。`manual` 时结果写在设置里。
    pub(crate) fn check_update(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update.rx.is_some() {
            return;
        }
        self.update.checking = true;
        if manual {
            self.update.status = Some("正在检查…".into());
        }
        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = nailong_core::http::Http::new("official")
                .and_then(|http| update::latest(&http))
                .map_err(|e| e.to_string());
            let _ = tx.send(UpdateMsg::Checked(result));
            ctx.request_repaint();
        });
    }

    /// 下载新版本，然后按装法接着做：Windows 记下等着换上，Linux 弹授权窗口装上。
    fn start_update(&mut self, ctx: &egui::Context) {
        if self.update.rx.is_some() {
            return;
        }
        let Some(release) = self.update.latest.clone() else {
            return;
        };
        let Some(asset) = update::asset_for_this_system(&release).cloned() else {
            self.update.error = Some("这个系统没有现成的安装包，请到 GitHub 页面下载".into());
            return;
        };
        let kind = self.update.kind;
        if kind == Install::Exe && !update::can_replace_exe() {
            self.update.error = Some("程序所在的文件夹不能写入，请到 GitHub 下载新版本".into());
            return;
        }
        self.update.error = None;
        self.update.manual = None;
        self.update.activity = Some(Activity::new(
            format!("Cairn {}", release.version),
            "准备下载",
        ));
        let cache = self.root.cache();
        let dir = update::download_dir(&cache);
        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress = tx.clone();
            let repaint = ctx.clone();
            let sink = from_fn(move |p| {
                let _ = progress.send(UpdateMsg::Progress(p));
                repaint.request_repaint();
            });
            let downloaded = nailong_core::http::Http::new("official")
                .and_then(|http| update::download(&http, &release, &asset, &dir, &sink));
            let msg = match (downloaded, kind) {
                (Err(e), _) => UpdateMsg::Failed(e.to_string()),
                (Ok(path), Install::Exe) => match update::stage(&cache, &release.version, &path) {
                    Ok(_) => UpdateMsg::Ready(release.version.clone()),
                    Err(e) => UpdateMsg::Failed(e.to_string()),
                },
                (Ok(path), Install::AptRepo | Install::Deb) => {
                    let _ = tx.send(UpdateMsg::Authorizing);
                    ctx.request_repaint();
                    match update::install_deb(&path) {
                        Ok(()) => UpdateMsg::Ready(release.version.clone()),
                        Err(e) => UpdateMsg::Manual(path, Some(e.to_string())),
                    }
                }
                (Ok(path), Install::Other) => UpdateMsg::Manual(path, None),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// 换成新版本并重新打开。
    fn restart_to_new(&mut self) {
        self.save_ui_config();
        let result = match self.update.kind {
            Install::Exe => update::apply_staged(&self.root.cache(), true).and_then(|applied| {
                applied
                    .map(|_| ())
                    .ok_or_else(|| nailong_core::Error::Install("下载的新版本不见了".into()))
            }),
            _ => update::relaunch(),
        };
        match result {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                self.update.ready = None;
                self.update.error = Some(format!("没能换到新版本：{e}"));
            }
        }
    }

    pub(crate) fn update_overlay(&mut self, ctx: &egui::Context) {
        if !self.update.open {
            return;
        }
        // 就绪的版本优先；Linux 上系统在后台装好时，可能还没查到更新说明。
        let Some(version) = self
            .update
            .ready
            .clone()
            .or_else(|| self.update.latest.as_ref().map(|r| r.version.clone()))
        else {
            self.update.open = false;
            return;
        };
        let notes = self
            .update
            .latest
            .as_ref()
            .filter(|release| release.version == version)
            .map(|release| plain_notes(&release.notes));
        let kind = self.update.kind;
        let playing = matches!(self.busy, Some(Task::Launch));
        let mut close = false;
        let mut skip = false;
        let mut start = false;
        let mut restart = false;
        let mut open_page = false;
        let mut reveal: Option<PathBuf> = None;
        let mut copy: Option<String> = None;
        let modal = egui::Modal::new(egui::Id::unique("update_modal"))
            .backdrop_color(t::veil(1.0))
            .frame(
                egui::Frame::new()
                    .fill(t::surface())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin::symmetric(24, 20)),
            )
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(
                    egui::RichText::new(format!("Cairn {version}"))
                        .size(24.0)
                        .family(display_family()),
                );
                ui.label(
                    egui::RichText::new(format!("现在是 {}", update::current()))
                        .size(13.0)
                        .color(t::sub()),
                );
                ui.add_space(12.0);
                egui::Frame::new()
                    .fill(t::bg())
                    .stroke(egui::Stroke::new(1.0, t::hairline()))
                    .corner_radius(motion::R)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                let empty = match &notes {
                                    Some(lines) => lines.iter().all(|l| l.trim().is_empty()),
                                    None => true,
                                };
                                if empty {
                                    ui.label(
                                        egui::RichText::new(if notes.is_some() {
                                            "这一版没有写更新说明"
                                        } else {
                                            "更新说明在 GitHub 上"
                                        })
                                        .size(13.0)
                                        .color(t::dim()),
                                    );
                                }
                                for line in notes.iter().flatten() {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(line).size(13.0))
                                            .wrap(),
                                    );
                                }
                            });
                    });
                ui.add_space(12.0);
                let line = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).size(13.0).color(color)).wrap(),
                    );
                };
                if let Some(activity) = &self.update.activity {
                    let p = &activity.progress;
                    motion::progress_bar(ui, p.fraction(), p.indeterminate, ui.input(|i| i.time));
                    ui.add_space(6.0);
                    let mut parts = vec![p.stage.clone()];
                    if p.bytes_total > 0 {
                        parts.push(progress_detail(p));
                    }
                    if let Some(speed) = activity.speed() {
                        parts.push(rate(speed));
                    }
                    if let Some(source) = activity.source_label() {
                        parts.push(format!("来自 {source}"));
                    }
                    line(ui, &parts.join(" · "), t::sub());
                } else if self.update.authorizing {
                    line(ui, "在系统弹出的窗口里输入密码", t::sub());
                } else if self.update.ready.is_some() {
                    line(
                        ui,
                        if kind == Install::Exe {
                            "已下载。关掉 Cairn 时自动换成新版本。"
                        } else {
                            "已装好。重启 Cairn 后生效。"
                        },
                        t::ok(),
                    );
                } else if let Some(path) = &self.update.manual {
                    let command = update::install_command(path);
                    line(ui, "已下载，校验通过。在终端里运行：", t::sub());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&command).monospace().size(12.5));
                    ui.horizontal(|ui| {
                        if action_link(ui, "复制命令").clicked() {
                            copy = Some(command.clone());
                        }
                        if action_link(ui, "在文件夹中显示").clicked() {
                            reveal = Some(path.clone());
                        }
                    });
                } else if self.update.system {
                    line(ui, "正在后台安装，装好后提示重启。", t::sub());
                }
                if let Some(error) = &self.update.error {
                    line(ui, error, t::danger());
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if action_link(ui, "在 GitHub 上看").clicked() {
                        open_page = true;
                    }
                    if self.update.ready.is_none() && action_link(ui, "跳过这个版本").clicked()
                    {
                        skip = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.update.ready.is_some() {
                            let response = ui.scope(|ui| {
                                motion::primary_sized(ui, "现在重启", !playing, 100.0, 32.0, 12.0)
                            });
                            if response.inner {
                                restart = true;
                            }
                            if playing {
                                response
                                    .response
                                    .on_hover_text("游戏运行中，退出游戏后再重启");
                            }
                        } else if self.update.manual.is_none() || kind.can_install() {
                            let label = match kind {
                                Install::Exe => "下载并更新",
                                Install::AptRepo | Install::Deb => "更新",
                                Install::Other => "下载 deb",
                            };
                            let label = if self.update.manual.is_some() {
                                "再试一次"
                            } else {
                                label
                            };
                            if motion::primary_sized(
                                ui,
                                label,
                                !self.update.busy(),
                                100.0,
                                32.0,
                                12.0,
                            ) {
                                start = true;
                            }
                        }
                        ui.add_space(6.0);
                        if motion::secondary_sized(ui, "以后再说", true, 80.0, 32.0, 12.0).clicked()
                        {
                            close = true;
                        }
                    });
                });
            });
        if modal.backdrop_response.clicked() || escape_pressed(ctx) {
            close = true;
        }
        if let Some(text) = copy {
            ctx.copy_text(text);
        }
        if let Some(path) = reveal {
            reveal_file(&path);
        }
        if open_page {
            open_url(&update::release_page(&version));
        }
        if start {
            self.start_update(ctx);
        }
        if restart {
            self.restart_to_new();
        }
        if skip {
            self.prefs.update_skip = version.clone();
            self.save_ui_config();
            close = true;
        }
        if close {
            self.update.open = false;
        }
    }
}

/// 侧栏底部的更新提示：有新版本，或者新版本已就绪只差重启。返回是否被点了。
pub(crate) fn update_pill(ui: &mut egui::Ui, version: &str, ready: bool, wide: bool) -> bool {
    let text = match (wide, ready) {
        (true, false) => format!("新版本 {version}"),
        (true, true) => format!("重启更新到 {version}"),
        // 窄侧栏放不下两个字，都用「新」，悬停时写清楚。
        (false, _) => "新".to_string(),
    };
    let response = ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.0).color(t::accent()))
            .fill(t::tint(t::accent(), 0.10))
            .stroke(egui::Stroke::new(1.0, t::tint(t::accent(), 0.45)))
            .corner_radius(motion::R)
            .min_size(egui::vec2(ui.available_width(), 30.0)),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
        .on_hover_text(if ready {
            format!("Cairn {version} 已就绪，重启后生效")
        } else {
            format!("Cairn {version} 可以更新了")
        })
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_read_as_plain_text() {
        let notes = "## 改了什么\n* **启动参数**看得见\n- 截图墙\n\nCo-Authored-By: someone";
        assert_eq!(
            plain_notes(notes),
            ["改了什么", "· 启动参数看得见", "· 截图墙", ""]
        );
    }

    #[test]
    fn skipped_versions_have_no_badge() {
        let state = UpdateState {
            latest: Some(Release {
                version: "9.9.9".into(),
                notes: String::new(),
                page: String::new(),
                assets: Vec::new(),
            }),
            ..Default::default()
        };
        assert_eq!(state.badge(""), Some(("9.9.9".into(), false)));
        assert_eq!(state.badge("9.9.9"), None);
        // 已经就绪的版本不受「跳过」影响。
        let state = UpdateState {
            ready: Some("9.9.9".into()),
            ..state
        };
        assert_eq!(state.badge("9.9.9"), Some(("9.9.9".into(), true)));
    }
}
