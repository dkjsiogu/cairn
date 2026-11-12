//! 界面自己的设置，存在数据目录的 `ui.json`：外观、全局启动参数、下载源、上次选的版本、窗口大小。
//!
//! 文件按键读写，不认识的键原样留着：旧版本写的文件新版本能读，新版本写的旧版本也不会读坏。

use crate::prelude::*;
use serde_json::{Value, json};

/// 界面缩放可选的几档。
pub(crate) const ZOOMS: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

pub(crate) struct UiPrefs {
    pub(crate) username: String,
    pub(crate) memory: Option<u32>,
    pub(crate) gc: tuning::GcChoice,
    pub(crate) jvm_args: String,
    pub(crate) reduce_motion: bool,
    pub(crate) theme: t::Preference,
    /// 上次检查更新的时间（Unix 秒）。
    pub(crate) update_checked: u64,
    /// 玩家选了「跳过这个版本」的版本号。
    pub(crate) update_skip: String,
    /// 下载源：`official` 官方优先（连不上自动换镜像），`bmcl` 镜像优先。
    pub(crate) source: String,
    /// 截图时把地点和人物写进图片本身；关掉就只记在 `shots.json`。
    pub(crate) embed_shots: bool,
    /// 打开时发现新版本就自动装：Windows 后台下载、关掉时换上；Linux 后台启动系统的更新服务。
    pub(crate) auto_update: bool,
    /// 上次选中的版本，下次打开还是它。
    pub(crate) selected: Option<String>,
    /// 游戏窗口出来后把启动器收到任务栏，游戏退出再放回来。
    pub(crate) minimize_on_launch: bool,
    /// 界面缩放，1.0 是原样。
    pub(crate) zoom: f32,
    /// 上次关掉时的窗口大小。
    pub(crate) window: Option<[f32; 2]>,
    pub(crate) maximized: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            username: "Player".into(),
            memory: None,
            gc: tuning::GcChoice::Auto,
            jvm_args: String::new(),
            reduce_motion: false,
            theme: t::Preference::System,
            update_checked: 0,
            update_skip: String::new(),
            source: "official".into(),
            embed_shots: true,
            auto_update: true,
            selected: None,
            minimize_on_launch: false,
            zoom: 1.0,
            window: None,
            maximized: false,
        }
    }
}

fn path(root: &Root) -> PathBuf {
    root.dir().join("ui.json")
}

impl UiPrefs {
    pub(crate) fn load(root: &Root) -> Self {
        std::fs::read(path(root))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .map(|value| Self::from_value(&value))
            .unwrap_or_default()
    }

    pub(crate) fn from_value(value: &Value) -> Self {
        let defaults = Self::default();
        let text = |key: &str| value.get(key).and_then(Value::as_str);
        let flag =
            |key: &str, default: bool| value.get(key).and_then(Value::as_bool).unwrap_or(default);
        // 0.33 及以前没有 "gc"，那时的 4096 是默认值而不是用户选的，迁移成自动。
        let legacy = value.get("gc").is_none();
        Self {
            username: text("username")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&defaults.username)
                .to_string(),
            memory: value
                .get("memory")
                .and_then(Value::as_u64)
                .filter(|&mb| !(legacy && mb == 4096))
                .map(|mb| mb.clamp(1024, 16384) as u32),
            gc: text("gc")
                .map(tuning::GcChoice::from_key)
                .unwrap_or_default(),
            jvm_args: text("jvm_args").unwrap_or_default().to_string(),
            reduce_motion: flag("reduce_motion", false),
            theme: text("theme")
                .map(t::Preference::from_key)
                .unwrap_or(t::Preference::System),
            update_checked: value
                .get("update_checked")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            update_skip: text("update_skip").unwrap_or_default().to_string(),
            source: match text("source") {
                Some("bmcl") => "bmcl".into(),
                _ => "official".into(),
            },
            embed_shots: flag("shots_embed", true),
            auto_update: flag("auto_update", true),
            selected: text("selected")
                .filter(|name| !name.is_empty())
                .map(str::to_string),
            minimize_on_launch: flag("minimize_on_launch", false),
            zoom: value
                .get("zoom")
                .and_then(Value::as_f64)
                .map(|zoom| nearest_zoom(zoom as f32))
                .unwrap_or(1.0),
            window: value.get("window").and_then(|size| {
                let width = size.get(0)?.as_f64()? as f32;
                let height = size.get(1)?.as_f64()? as f32;
                // 坏掉的数字（负数、几万像素）不用。
                ((600.0..=10000.0).contains(&width) && (400.0..=10000.0).contains(&height))
                    .then_some([width, height])
            }),
            maximized: flag("maximized", false),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({
            "username": self.username,
            "memory": self.memory,
            "gc": self.gc.key(),
            "jvm_args": self.jvm_args.trim(),
            "reduce_motion": self.reduce_motion,
            "theme": self.theme.key(),
            "update_checked": self.update_checked,
            "update_skip": self.update_skip,
            "source": self.source,
            "shots_embed": self.embed_shots,
            "auto_update": self.auto_update,
            "selected": self.selected,
            "minimize_on_launch": self.minimize_on_launch,
            "zoom": self.zoom,
            "window": self.window,
            "maximized": self.maximized,
        })
    }

    /// 写回 `ui.json`。先读出原来的，只换我们认识的键；先写临时文件再改名，断电也不会留半个文件。
    pub(crate) fn save(&self, root: &Root) {
        let path = path(root);
        let mut merged = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if let (Some(target), Value::Object(ours)) = (merged.as_object_mut(), self.to_value()) {
            target.extend(ours);
        }
        let Ok(bytes) = serde_json::to_vec_pretty(&merged) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 离 `zoom` 最近的一档。
pub(crate) fn nearest_zoom(zoom: f32) -> f32 {
    ZOOMS
        .into_iter()
        .min_by(|a, b| (a - zoom).abs().total_cmp(&(b - zoom).abs()))
        .unwrap_or(1.0)
}

impl App {
    pub(crate) fn save_ui_config(&self) {
        self.prefs.save(&self.root);
    }

    /// 选中的版本变了就记下来，下次打开还是它。
    pub(crate) fn remember_selection(&mut self) {
        if self.selected != self.prefs.selected && self.selected.is_some() {
            self.prefs.selected = self.selected.clone();
            self.save_ui_config();
        }
    }
}
