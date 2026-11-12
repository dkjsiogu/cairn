//! 导出整合包：从一个版本文件夹得到一个最小的 `.mrpack`。
//!
//! 不是把整个版本文件夹打包：
//!
//! - `mods/`、`resourcepacks/`、`shaderpacks/` 里能在 Modrinth 或 CurseForge 下到的文件，
//!   只在清单里记下载地址和哈希，安装时由安装的人自己去下。
//! - 公开渠道找不到的（自己做的、改过的、作者不让第三方分发的），连同 config、kubejs、
//!   脚本、世界存档这些，打包进 `overrides/`。
//!
//! `.mrpack` 是 Modrinth 的整合包格式，Cairn、Prism、HMCL、PCL 都能导入。
//! 见 `docs/design/28-export.md`。

use crate::cancel;
use crate::error::{Error, Result, io};
use crate::http::Http;
use crate::paths::Root;
use crate::progress::{Progress, Sink};
use crate::versions::{self, Loader};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
const MCIM: &str = "https://mod.mcimirror.top";
/// CurseForge 上 Minecraft 的游戏编号。
const CURSEFORGE_MINECRAFT: u32 = 432;

/// 这些文件夹里的文件先去网上找，找得到就不打包。
const LOOKUP_DIRS: [&str; 3] = ["mods", "resourcepacks", "shaderpacks"];

/// 从来不导出：日志、缓存、备份、启动器自己的文件、版本文件本身。没开版本隔离的版本，游戏目录
/// 就是 `.minecraft`，这时 versions、libraries、assets 和启动器的设置文件也在这一层，一样不导出。
const SKIP: [&str; 34] = [
    "versions",
    "libraries",
    "assets",
    "instances",
    "runtime",
    "authlib-injector",
    "accounts.json",
    "ui.json",
    "launch-settings.json",
    "backups.json",
    "bench.json",
    "shots.json",
    "learned.json",
    "launcher_accounts.json",
    "logs",
    "crash-reports",
    "backups",
    "natives",
    "screenshots",
    "downloads",
    "cache",
    ".cache",
    ".fabric",
    ".mixin.out",
    ".connector",
    "PCL",
    "usercache.json",
    "usernamecache.json",
    "launcher_profiles.json",
    "hmclversion.cfg",
    ".nailong-content.json",
    ".nailong-isolated",
    ".nailong-incomplete",
    "command_history.txt",
];

/// 列出来但默认不勾：小地图记录、录像、蓝图、服务器列表这类玩家自己的东西。按名字开头比。
const DEFAULT_OFF: [&str; 8] = [
    "journeymap",
    "xaero",
    "replay_recordings",
    "schematics",
    "servers.dat",
    "screenshots",
    // KubeJS 的 local/ 放的是这台电脑自己的设置。
    "local",
    "distant_horizons",
];

/// 导出弹窗里的一项：版本文件夹里的一个文件夹、文件，或者一个世界。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// 相对版本文件夹的路径：`mods`、`options.txt`、`saves/新的世界`。
    pub path: String,
    pub world: bool,
    pub folder: bool,
    pub size: u64,
    pub files: usize,
    /// 默认勾不勾。世界、地图记录这类默认不勾。
    pub default_on: bool,
}

/// 版本文件夹里能导出的东西。
pub fn items(game_dir: &Path, version_name: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(game_dir) else {
        return out;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if skipped(&name, version_name) {
            continue;
        }
        if name == "saves" && path.is_dir() {
            // 每个世界单独一项，默认不勾。
            let mut worlds: Vec<_> = std::fs::read_dir(&path)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|w| w.path().is_dir())
                .collect();
            worlds.sort_by_key(|w| w.file_name());
            for world in worlds {
                let (size, files) = measure(&world.path());
                out.push(Item {
                    path: format!("saves/{}", world.file_name().to_string_lossy()),
                    world: true,
                    folder: true,
                    size,
                    files,
                    default_on: false,
                });
            }
            continue;
        }
        let (size, files) = measure(&path);
        if files == 0 {
            continue;
        }
        out.push(Item {
            folder: path.is_dir(),
            default_on: !default_off(&name),
            path: name,
            world: false,
            size,
            files,
        });
    }
    out
}

/// 默认不勾的：按名字开头比，不分大小写（`XaeroWaypoints_BACKUP240807` 也算）。
fn default_off(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    DEFAULT_OFF.iter().any(|prefix| lower.starts_with(prefix))
}

fn skipped(name: &str, version_name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SKIP.contains(&name)
        // 以点开头的多是 Mod 的缓存（.fabric、.mixin.out、.sable/natives…）。
        || name.starts_with('.')
        // 名字里带 cache 的是缓存，装好以后会自己生成。
        || lower.contains("cache")
        || name == format!("{version_name}.json")
        || name == format!("{version_name}.jar")
        || name.ends_with(".log")
        || name.starts_with("hs_err_pid")
        || name.ends_with(".tmp")
        || name == ".DS_Store"
        || name == "Thumbs.db"
}

/// 一个文件或文件夹的总大小和文件数。
fn measure(path: &Path) -> (u64, usize) {
    if path.is_file() {
        return (path.metadata().map(|m| m.len()).unwrap_or(0), 1);
    }
    let mut size = 0;
    let mut files = 0;
    for file in walk(path) {
        size += file.metadata().map(|m| m.len()).unwrap_or(0);
        files += 1;
    }
    (size, files)
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name != ".DS_Store" && name != "Thumbs.db" {
                    out.push(entry.path());
                }
            }
        }
    }
    out.sort();
    out
}

/// 导出时填的东西。
#[derive(Debug, Clone)]
pub struct Options {
    pub name: String,
    /// 整合包自己的版本号，例如 `1.0.0`。
    pub version: String,
    pub summary: String,
    /// 勾上的项（[`Item::path`]）。
    pub include: Vec<String>,
}

/// 导出的结果。
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub path: PathBuf,
    /// 记了下载地址、安装时去下的文件数。
    pub online: usize,
    /// 打包进去的文件数。
    pub packed: usize,
    /// 压缩包的大小。
    pub bytes: u64,
    /// 本来想在网上找、但没找到而打包进去的 Mod、资源包、光影（文件名）。
    pub not_found: Vec<String>,
    /// 需要提醒的事，例如版本用了整合包自带的组件补丁。
    pub warnings: Vec<String>,
}

/// 一个要导出的文件。
struct File {
    /// 在整合包里的路径（相对游戏目录，`/` 分隔）。
    path: String,
    size: u64,
    /// 能在网上找到时的下载地址。
    url: Option<String>,
    sha1: String,
    sha512: String,
}

/// 去哪找文件：按 SHA-1 问 Modrinth，按指纹问 CurseForge。测试里换成假的，不连网。
struct Lookup<'a> {
    modrinth: &'a FindBySha1,
    curseforge: &'a FindByFingerprint,
}

/// 给一批 SHA-1，返回找到的（SHA-1，下载地址）。
type FindBySha1 = dyn Fn(&[String]) -> Vec<(String, String)>;
/// 给一批 CurseForge 指纹，返回找到的（指纹，下载地址）。
type FindByFingerprint = dyn Fn(&[u32]) -> Vec<(u32, String)>;

/// 导出 `version_name` 这个版本为 `dest`（`.mrpack`）。
pub fn export(
    root: &Root,
    version_name: &str,
    options: &Options,
    dest: &Path,
    sink: &Sink,
) -> Result<Report> {
    let lookup = Lookup {
        modrinth: &modrinth_urls,
        curseforge: &curseforge_urls,
    };
    export_with(root, version_name, options, dest, sink, &lookup)
}

fn export_with(
    root: &Root,
    version_name: &str,
    options: &Options,
    dest: &Path,
    sink: &Sink,
    lookup: &Lookup,
) -> Result<Report> {
    cancel::check()?;
    let game_dir = versions::game_dir(root, version_name);
    let (mc, loader, loader_version, warnings) = describe(root, version_name);

    // 1. 列出要导出的文件。
    (sink)(Progress::stage("读取文件"));
    let mut sources: Vec<(String, PathBuf)> = Vec::new();
    for include in &options.include {
        let relative = safe_relative(include)?;
        let path = game_dir.join(&relative);
        if path.is_file() {
            sources.push((include.clone(), path));
        } else if path.is_dir() {
            for file in walk(&path) {
                let inner = file
                    .strip_prefix(&game_dir)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                // 禁用的 Mod 不导出。
                if inner.starts_with("mods/") && inner.ends_with(".disabled") {
                    continue;
                }
                sources.push((inner, file));
            }
        }
    }
    sources.sort();
    sources.dedup_by(|a, b| a.0 == b.0);

    // 2. 能在网上找的先算哈希。
    let wanted: Vec<usize> = sources
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| looked_up(path))
        .map(|(index, _)| index)
        .collect();
    let total = wanted.len() as u64;
    let done = std::sync::atomic::AtomicU64::new(0);
    let hashed: Vec<Result<(String, String, u32, u64)>> = {
        use rayon::prelude::*;
        wanted
            .par_iter()
            .map(|&index| {
                cancel::check()?;
                let result = hash_file(&sources[index].1);
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                (sink)(Progress {
                    stage: "读取文件".into(),
                    files_done: n,
                    files_total: total,
                    current: Some(sources[index].0.clone()),
                    ..Default::default()
                });
                result
            })
            .collect()
    };
    let mut files: Vec<File> = Vec::new();
    let mut fingerprints: HashMap<u32, usize> = HashMap::new();
    for (&index, hashed) in wanted.iter().zip(hashed) {
        let (sha1, sha512, fingerprint, size) = hashed?;
        let (path, _) = &sources[index];
        fingerprints.insert(fingerprint, files.len());
        files.push(File {
            path: path.clone(),
            size,
            url: None,
            sha1,
            sha512,
        });
    }

    // 3. 去 Modrinth 找，找不到的再去 CurseForge。
    if !files.is_empty() {
        (sink)(Progress::stage("在 Modrinth 上查找"));
        let hashes: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
        for (sha1, url) in (lookup.modrinth)(&hashes) {
            if let Some(file) = files.iter_mut().find(|f| f.sha1 == sha1) {
                file.url = Some(url);
            }
        }
        let missing: Vec<u32> = fingerprints
            .iter()
            .filter(|(_, index)| files[**index].url.is_none())
            .map(|(fingerprint, _)| *fingerprint)
            .collect();
        if !missing.is_empty() {
            (sink)(Progress::stage("在 CurseForge 上查找"));
            for (fingerprint, url) in (lookup.curseforge)(&missing) {
                if let Some(&index) = fingerprints.get(&fingerprint) {
                    files[index].url = Some(url);
                }
            }
        }
    }
    cancel::check()?;

    // 4. 写压缩包：清单 + 找不到的和别的文件放进 overrides/。
    let mut report = Report {
        path: dest.to_path_buf(),
        warnings,
        ..Default::default()
    };
    let mut index_files = Vec::new();
    for file in &files {
        match &file.url {
            Some(url) => {
                report.online += 1;
                index_files.push(json!({
                    "path": file.path,
                    "hashes": { "sha1": file.sha1, "sha512": file.sha512 },
                    "env": { "client": "required", "server": "required" },
                    "downloads": [url],
                    "fileSize": file.size,
                }));
            }
            None => report.not_found.push(file.path.clone()),
        }
    }
    let online: std::collections::HashSet<&str> = files
        .iter()
        .filter(|f| f.url.is_some())
        .map(|f| f.path.as_str())
        .collect();
    let packed: Vec<&(String, PathBuf)> = sources
        .iter()
        .filter(|(path, _)| !online.contains(path.as_str()))
        .collect();
    report.packed = packed.len();

    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".into(), Value::String(mc));
    if let (Some(key), Some(version)) = (loader_key(loader), loader_version) {
        dependencies.insert(key.into(), Value::String(version));
    }
    let mut index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": options.version,
        "name": options.name,
        "files": index_files,
        "dependencies": dependencies,
    });
    if !options.summary.trim().is_empty() {
        index["summary"] = Value::String(options.summary.trim().to_string());
    }

    let tmp = dest.with_extension("mrpack.part");
    let written = write_pack(&tmp, &index, &packed, sink);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(dest, e)
    })?;
    report.bytes = dest.metadata().map(|m| m.len()).unwrap_or(0);
    (sink)(Progress {
        stage: "导出完成".into(),
        files_done: 1,
        files_total: 1,
        bytes_done: 1,
        bytes_total: 1,
        ..Default::default()
    });
    Ok(report)
}

/// 这个文件要不要先去网上找：mods、resourcepacks、shaderpacks 第一层的 jar 和 zip。
fn looked_up(path: &str) -> bool {
    LOOKUP_DIRS.iter().any(|dir| {
        path.strip_prefix(dir)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|name| {
                !name.contains('/') && (name.ends_with(".jar") || name.ends_with(".zip"))
            })
    })
}

/// MC 版本、加载器、加载器版本，以及要提醒的事。
fn describe(root: &Root, name: &str) -> (String, Loader, Option<String>, Vec<String>) {
    let mut warnings = Vec::new();
    match versions::meta(root, name) {
        Some(meta) => {
            if meta.profile.starts_with("prism-") {
                warnings.push(
                    "这个版本用了整合包自带的组件补丁（比如换了 LWJGL），.mrpack 只记得住 MC 和加载器版本，装出来可能不一样"
                        .into(),
                );
            }
            (meta.mc, meta.loader, meta.loader_version, warnings)
        }
        None => {
            let (mc, loader, version) = versions::detect(root, name);
            (mc, loader, version, warnings)
        }
    }
}

fn loader_key(loader: Loader) -> Option<&'static str> {
    match loader {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    }
}

fn safe_relative(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if path.is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Plain(format!("不能导出这个路径：{path}")));
    }
    Ok(relative.to_path_buf())
}

/// SHA-1、SHA-512、CurseForge 指纹和大小。
fn hash_file(path: &Path) -> Result<(String, String, u32, u64)> {
    let bytes = std::fs::read(path).map_err(|e| io(path, e))?;
    let sha1 = crate::http::hex(&Sha1::digest(&bytes));
    let sha512 = crate::http::hex(&Sha512::digest(&bytes));
    Ok((
        sha1,
        sha512,
        curseforge_fingerprint(&bytes),
        bytes.len() as u64,
    ))
}

/// CurseForge 的文件指纹：去掉空白字符（制表、换行、回车、空格）后算 MurmurHash2，种子 1。
pub fn curseforge_fingerprint(bytes: &[u8]) -> u32 {
    let data: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    const M: u32 = 0x5bd1_e995;
    let mut h: u32 = 1 ^ (data.len() as u32);
    let (chunks, rest) = data.as_chunks::<4>();
    for chunk in chunks {
        let mut k = u32::from_le_bytes(*chunk);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    if !rest.is_empty() {
        if rest.len() >= 3 {
            h ^= (rest[2] as u32) << 16;
        }
        if rest.len() >= 2 {
            h ^= (rest[1] as u32) << 8;
        }
        h ^= rest[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// 按 SHA-1 在 Modrinth 上找文件，返回（SHA-1，下载地址）。官方连不上时问 MCIM 镜像。
fn modrinth_urls(hashes: &[String]) -> Vec<(String, String)> {
    let Ok(http) = Http::new("official") else {
        return Vec::new();
    };
    let body = json!({ "hashes": hashes, "algorithm": "sha1" });
    let value = [
        format!("{MODRINTH_API}/version_files"),
        format!("{MCIM}/modrinth/v2/version_files"),
    ]
    .iter()
    .find_map(|url| match http.post_json(url, &body, &[]) {
        Ok((200, value)) if value.is_object() => Some(value),
        _ => None,
    });
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(sha1, version)| {
            let file = version.get("files")?.as_array()?.iter().find(|file| {
                file.pointer("/hashes/sha1").and_then(Value::as_str) == Some(sha1.as_str())
            })?;
            Some((sha1.clone(), file.get("url")?.as_str()?.to_string()))
        })
        .collect()
}

/// 按指纹在 CurseForge 上找文件，返回（指纹，下载地址）。作者不让第三方下载的没有地址，
/// 就不返回（会打包进去）。有 API key 时问官方；没有或连不上时问 MCIM 镜像，镜像不带 key。
fn curseforge_urls(fingerprints: &[u32]) -> Vec<(u32, String)> {
    let Ok(http) = Http::new("official") else {
        return Vec::new();
    };
    let body = json!({ "fingerprints": fingerprints });
    let key = crate::builtin::curseforge_api_key();
    let mut value = None;
    if let Some(key) = &key {
        let headers = [("x-api-key", key.as_str()), ("Accept", "application/json")];
        if let Ok((200, v)) = http.post_json(
            &format!("{CURSEFORGE_API}/fingerprints/{CURSEFORGE_MINECRAFT}"),
            &body,
            &headers,
        ) {
            value = Some(v);
        }
    }
    if value.is_none()
        && let Ok((200, v)) =
            http.post_json(&format!("{MCIM}/curseforge/v1/fingerprints"), &body, &[])
    {
        value = Some(v);
    }
    let Some(value) = value else {
        return Vec::new();
    };
    value
        .pointer("/data/exactMatches")
        .and_then(Value::as_array)
        .map(|matches| {
            matches
                .iter()
                .filter_map(|m| {
                    let file = m.get("file")?;
                    let fingerprint = file.get("fileFingerprint")?.as_u64()? as u32;
                    let url = file.get("downloadUrl")?.as_str()?;
                    (!url.is_empty()).then(|| (fingerprint, url.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 写 `.mrpack`：先写清单，再把文件放进 `overrides/`。jar 和 zip 本来就是压缩过的，原样存；
/// 别的（配置、存档）压缩。
fn write_pack(
    dest: &Path,
    index: &Value,
    packed: &[&(String, PathBuf)],
    sink: &Sink,
) -> Result<()> {
    use zip::write::SimpleFileOptions;
    let file = std::fs::File::create(dest).map_err(|e| io(dest, e))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    let zip_err = |e: zip::result::ZipError| Error::Install(format!("写整合包失败：{e}"));
    zip.start_file("modrinth.index.json", deflated)
        .map_err(zip_err)?;
    let text = serde_json::to_vec_pretty(index).map_err(|e| Error::Install(e.to_string()))?;
    zip.write_all(&text).map_err(|e| io(dest, e))?;

    let total: u64 = packed
        .iter()
        .map(|(_, source)| source.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    let mut done = 0u64;
    for (index, (path, source)) in packed.iter().enumerate() {
        cancel::check()?;
        let size = source.metadata().map(|m| m.len()).unwrap_or(0);
        let lower = path.to_ascii_lowercase();
        let already_compressed = [".jar", ".zip", ".png", ".jpg", ".ogg", ".gz", ".mca"]
            .iter()
            .any(|ext| lower.ends_with(ext));
        let options = if already_compressed {
            stored
        } else {
            deflated.large_file(size > u32::MAX as u64 / 2)
        };
        zip.start_file(format!("overrides/{path}"), options)
            .map_err(zip_err)?;
        let mut input = std::fs::File::open(source).map_err(|e| io(source, e))?;
        std::io::copy(&mut input, &mut zip).map_err(|e| io(source, e))?;
        done += size;
        (sink)(Progress {
            stage: "打包".into(),
            files_done: index as u64 + 1,
            files_total: packed.len() as u64,
            bytes_done: done,
            bytes_total: total,
            current: Some(path.clone()),
            ..Default::default()
        });
    }
    let mut inner = zip.finish().map_err(zip_err)?;
    inner.flush().map_err(|e| io(dest, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!(
            "cairn-export-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Root::new(dir)
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn curseforge_fingerprint_matches_known_values() {
        // 和 CurseForge 的算法一致：空白字符不算，种子 1。
        assert_eq!(
            curseforge_fingerprint(b""),
            curseforge_fingerprint(b" \n\t\r")
        );
        assert_eq!(
            curseforge_fingerprint(b"ab cd"),
            curseforge_fingerprint(b"abcd")
        );
        assert_ne!(
            curseforge_fingerprint(b"abcd"),
            curseforge_fingerprint(b"abce")
        );
        // MurmurHash2（种子 1）的参考值。
        assert_eq!(
            curseforge_fingerprint(b"abcd"),
            murmur2_reference(b"abcd", 1)
        );
        assert_eq!(
            curseforge_fingerprint(b"abcdefg"),
            murmur2_reference(b"abcdefg", 1)
        );
    }

    /// 照着 MurmurHash2 的 C 参考实现逐字写的，用来核对。
    fn murmur2_reference(data: &[u8], seed: u32) -> u32 {
        let m: u32 = 0x5bd1e995;
        let r = 24;
        let mut len = data.len();
        let mut h: u32 = seed ^ (len as u32);
        let mut i = 0;
        while len >= 4 {
            let mut k = u32::from(data[i])
                | (u32::from(data[i + 1]) << 8)
                | (u32::from(data[i + 2]) << 16)
                | (u32::from(data[i + 3]) << 24);
            k = k.wrapping_mul(m);
            k ^= k >> r;
            k = k.wrapping_mul(m);
            h = h.wrapping_mul(m);
            h ^= k;
            i += 4;
            len -= 4;
        }
        match len {
            3 => {
                h ^= u32::from(data[i + 2]) << 16;
                h ^= u32::from(data[i + 1]) << 8;
                h ^= u32::from(data[i]);
                h = h.wrapping_mul(m);
            }
            2 => {
                h ^= u32::from(data[i + 1]) << 8;
                h ^= u32::from(data[i]);
                h = h.wrapping_mul(m);
            }
            1 => {
                h ^= u32::from(data[i]);
                h = h.wrapping_mul(m);
            }
            _ => {}
        }
        h ^= h >> 13;
        h = h.wrapping_mul(m);
        h ^= h >> 15;
        h
    }

    #[test]
    fn items_list_folders_and_each_world_but_skip_noise() {
        let root = temp_root("items");
        let dir = root.version_dir("生存");
        write(&dir.join("生存.json"), b"{}");
        write(&dir.join("生存.jar"), b"jar");
        write(&dir.join("mods/a.jar"), b"a");
        write(&dir.join("config/a.toml"), b"x");
        write(&dir.join("kubejs/server_scripts/a.js"), b"js");
        write(&dir.join("saves/家/level.dat"), b"w");
        write(&dir.join("saves/测试/level.dat"), b"w");
        write(&dir.join("logs/latest.log"), b"log");
        write(&dir.join("journeymap/data.dat"), b"map");
        write(&dir.join("options.txt"), b"o");
        write(&dir.join("backups/instance/x"), b"b");
        write(&dir.join("dynamic-data-pack-cache/x"), b"c");
        write(&dir.join(".sable/natives/x.so"), b"n");
        write(&dir.join("XaeroWaypoints_BACKUP240807/a.txt"), b"w");
        write(&dir.join("local/kubejs/dev.properties"), b"l");
        let items = items(&dir, "生存");
        let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "XaeroWaypoints_BACKUP240807",
                "config",
                "journeymap",
                "kubejs",
                "local",
                "mods",
                "options.txt",
                "saves/家",
                "saves/测试"
            ]
        );
        let on: Vec<&str> = items
            .iter()
            .filter(|i| i.default_on)
            .map(|i| i.path.as_str())
            .collect();
        assert_eq!(on, ["config", "kubejs", "mods", "options.txt"]);
        assert!(
            items
                .iter()
                .filter(|i| i.world)
                .all(|i| i.folder && !i.default_on)
        );
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn only_first_level_mods_packs_and_shaders_are_looked_up() {
        assert!(looked_up("mods/a.jar"));
        assert!(looked_up("resourcepacks/b.zip"));
        assert!(looked_up("shaderpacks/c.zip"));
        assert!(!looked_up("mods/sub/a.jar"));
        assert!(!looked_up("mods/a.jar.disabled"));
        assert!(!looked_up("config/a.jar"));
        assert!(!looked_up("modsx/a.jar"));
    }

    #[test]
    fn links_what_is_online_and_packs_the_rest() {
        let root = temp_root("pack");
        let dir = root.version_dir("生存");
        write(
            &dir.join("生存.json"),
            serde_json::json!({
                "id": "生存",
                "cairn": {"mc": "1.21.1", "loader": "fabric", "loaderVersion": "0.16.10",
                          "profile": "fabric-loader-0.16.10-1.21.1"}
            })
            .to_string()
            .as_bytes(),
        );
        // 自己做的 Mod：网上找不到，打包进去。
        write(
            &dir.join("mods/my-own-mod-cairn-test.jar"),
            b"not on any site 0x5f3759df",
        );
        write(&dir.join("mods/off.jar.disabled"), b"disabled");
        write(&dir.join("config/a.toml"), b"x = 1");
        write(&dir.join("saves/家/level.dat"), b"world");
        let dest = root.dir().join("out.mrpack");
        let options = Options {
            name: "测试包".into(),
            version: "1.0.0".into(),
            summary: String::new(),
            include: vec!["mods".into(), "config".into(), "saves/家".into()],
        };
        // 公开的 Mod：假装 Modrinth 上有。
        write(&dir.join("mods/public.jar"), b"public mod");
        let public_sha1 = crate::http::hex(&Sha1::digest(b"public mod"));
        let wanted = public_sha1.clone();
        let modrinth = move |hashes: &[String]| -> Vec<(String, String)> {
            hashes
                .iter()
                .filter(|h| **h == wanted)
                .map(|h| {
                    (
                        h.clone(),
                        "https://cdn.modrinth.com/data/x/public.jar".to_string(),
                    )
                })
                .collect()
        };
        let curseforge = |_: &[u32]| -> Vec<(u32, String)> { Vec::new() };
        let lookup = Lookup {
            modrinth: &modrinth,
            curseforge: &curseforge,
        };
        let report = export_with(
            &root,
            "生存",
            &options,
            &dest,
            &crate::progress::noop(),
            &lookup,
        )
        .unwrap();
        assert_eq!(report.online, 1);
        assert_eq!(report.packed, 3);
        assert_eq!(report.not_found, ["mods/my-own-mod-cairn-test.jar"]);
        assert!(report.bytes > 0);

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "modrinth.index.json",
                "overrides/config/a.toml",
                "overrides/mods/my-own-mod-cairn-test.jar",
                "overrides/saves/家/level.dat",
            ]
        );
        let index: Value = {
            let mut text = String::new();
            std::io::Read::read_to_string(
                &mut zip.by_name("modrinth.index.json").unwrap(),
                &mut text,
            )
            .unwrap();
            serde_json::from_str(&text).unwrap()
        };
        assert_eq!(index["name"], "测试包");
        let files = index["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["path"], "mods/public.jar");
        assert_eq!(files[0]["hashes"]["sha1"], public_sha1);
        assert_eq!(
            files[0]["downloads"][0],
            "https://cdn.modrinth.com/data/x/public.jar"
        );
        assert_eq!(index["dependencies"]["minecraft"], "1.21.1");
        assert_eq!(index["dependencies"]["fabric-loader"], "0.16.10");
        assert!(!dest.with_extension("mrpack.part").exists());
        let _ = std::fs::remove_dir_all(root.dir());
    }
}
