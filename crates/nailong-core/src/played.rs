//! 每个版本玩了多久、上次什么时候玩的。存在数据目录的 `played.json`。
//!
//! 界面用它排「最近玩过」、显示游玩时长；崩溃后用 `last_good` 找出「上次正常游戏之后改过什么」。

use crate::error::{Error, Result, io};
use crate::paths::Root;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const STORE: &str = "played.json";

/// 一局算「正常」要满足的条件：正常退出且至少玩了这么久，或者不管怎么退出都玩够了 `LONG` 秒。
const SHORT: u64 = 30;
const LONG: u64 = 600;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    /// 一共玩了多少秒。
    #[serde(default)]
    pub seconds: u64,
    /// 启动过几次。
    #[serde(default)]
    pub launches: u32,
    /// 上次开始玩的时间（Unix 秒）。
    #[serde(default)]
    pub last: u64,
    /// 上次正常玩完的那一局是什么时候开始的；没有过就是 0。
    #[serde(default)]
    pub last_good: u64,
}

pub type Played = BTreeMap<String, Stat>;

/// 这一局算不算正常：闪退和启动就崩的不算。
pub fn is_good(code: i32, seconds: u64) -> bool {
    (code == 0 && seconds >= SHORT) || seconds >= LONG
}

pub fn load(root: &Root) -> Played {
    std::fs::read(root.dir().join(STORE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(root: &Root, all: &Played) -> Result<()> {
    let path = root.dir().join(STORE);
    let bytes = serde_json::to_vec_pretty(all).map_err(|e| Error::Plain(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
}

/// 记下一局：什么时候开始、玩了多久、怎么退出的。返回这个版本更新后的记录。
pub fn record(root: &Root, name: &str, started: u64, seconds: u64, code: i32) -> Result<Stat> {
    let mut all = load(root);
    let stat = all.entry(name.to_string()).or_default();
    stat.seconds += seconds;
    stat.launches += 1;
    stat.last = stat.last.max(started);
    if is_good(code, seconds) {
        stat.last_good = stat.last_good.max(started);
    }
    let stat = stat.clone();
    save(root, &all)?;
    Ok(stat)
}

/// 版本改名时把记录带过去。
pub fn rename(root: &Root, from: &str, to: &str) {
    let mut all = load(root);
    if let Some(stat) = all.remove(from) {
        all.insert(to.to_string(), stat);
        let _ = save(root, &all);
    }
}

/// 版本删掉后不留记录。
pub fn forget(root: &Root, name: &str) {
    let mut all = load(root);
    if all.remove(name).is_some() {
        let _ = save(root, &all);
    }
}

/// 「12 小时」「35 分钟」这样的时长；不到一分钟是「不到 1 分钟」。
pub fn human(seconds: u64) -> String {
    match seconds {
        0..=59 => "不到 1 分钟".into(),
        60..=3599 => format!("{} 分钟", seconds / 60),
        3600..=359_999 => {
            let hours = seconds as f64 / 3600.0;
            if hours < 10.0 && (hours * 10.0).round() % 10.0 != 0.0 {
                format!("{hours:.1} 小时")
            } else {
                format!("{} 小时", hours.round() as u64)
            }
        }
        _ => format!("{} 小时", seconds / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> (Root, std::path::PathBuf) {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("cairn-played-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (Root::new(&dir), dir)
    }

    #[test]
    fn records_add_up_and_only_good_sessions_move_last_good() {
        let (root, dir) = temp_root("record");
        // 玩了一个小时，正常退出。
        let stat = record(&root, "pack", 1000, 3600, 0).unwrap();
        assert_eq!(
            (stat.seconds, stat.launches, stat.last, stat.last_good),
            (3600, 1, 1000, 1000)
        );
        // 加了个 Mod 之后启动就崩。
        let stat = record(&root, "pack", 9000, 12, 1).unwrap();
        assert_eq!(
            (stat.seconds, stat.launches, stat.last, stat.last_good),
            (3612, 2, 9000, 1000)
        );
        // 正常退出但只开了几秒（点错了）：不算玩过一局。
        let stat = record(&root, "pack", 9500, 5, 0).unwrap();
        assert_eq!(stat.last_good, 1000);
        // 玩了很久才崩：内容本身没问题。
        let stat = record(&root, "pack", 10_000, 1200, -1).unwrap();
        assert_eq!(stat.last_good, 10_000);
        assert_eq!(load(&root).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rename_and_forget_follow_the_version() {
        let (root, dir) = temp_root("rename");
        record(&root, "old", 10, 100, 0).unwrap();
        rename(&root, "old", "new");
        let all = load(&root);
        assert!(all.contains_key("new") && !all.contains_key("old"));
        forget(&root, "new");
        assert!(load(&root).is_empty());
        // 没有记录的版本改名、删除都不出错。
        rename(&root, "ghost", "other");
        forget(&root, "ghost");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human(20), "不到 1 分钟");
        assert_eq!(human(35 * 60), "35 分钟");
        assert_eq!(human(3600), "1 小时");
        assert_eq!(human(5400), "1.5 小时");
        assert_eq!(human(12 * 3600 + 1200), "12 小时");
        assert_eq!(human(200 * 3600), "200 小时");
    }
}
