//! 版本：每个版本是 `versions/<名字>/` 一个文件夹，自带完整的版本 JSON（没有 `inheritsFrom`）、
//! 客户端 jar 和这个版本的全部游戏文件（mods、saves、config…）。版本之间没有依赖。
//!
//! 安装时先把原版和加载器的 profile 装进 `cache/cairn/profiles/`（`Root::store`），
//! 再合并成一个完整的 JSON 写进版本文件夹，并在 JSON 里记下 `cairn`：MC 版本、加载器、
//! 用的哪个 profile。见 `docs/design/27-version-folders.md`。

use crate::error::{Error, Result, io};
use crate::install::{fabric, forge, neoforge, vanilla};
use crate::manifest::{self, Context, VersionJson};
use crate::paths::{Root, write_atomic};
use crate::progress::Sink;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 版本 JSON 里 Cairn 自己那一项的键。
const META_KEY: &str = "cairn";
/// 名字最长多少个字。
const NAME_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
}

impl Loader {
    /// 默认名里用的写法：`1.21.1 NeoForge`。
    pub fn label(self) -> &'static str {
        match self {
            Loader::Vanilla => "",
            Loader::Fabric => "Fabric",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }
}

/// 整合包的来历。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    /// `mrpack`、`curseforge`、`prism`、`mcbbs`、`hmcl`、`atl`。
    pub format: String,
    /// 导入时的文件名。
    #[serde(default)]
    pub source: String,
}

/// 写在版本 JSON 的 `cairn` 里：这个版本是什么、怎么装的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub mc: String,
    pub loader: Loader,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// 合并前用的 profile（在 `cache/cairn/profiles/` 里）。
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<Pack>,
}

/// 读一个版本的 `cairn` 记录；别的启动器装的没有。
pub fn meta(root: &Root, name: &str) -> Option<Meta> {
    let bytes = std::fs::read(root.version_json(name)).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    serde_json::from_value(value.get(META_KEY)?.clone()).ok()
}

/// 检查名字能不能用，返回去掉首尾空白后的名字。
pub fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    let bad = |msg: &str| Err(Error::Plain(msg.to_string()));
    if name.is_empty() {
        return bad("名字不能为空");
    }
    if name == "." || name == ".." {
        return bad("名字不能是 . 或 ..");
    }
    if name.chars().count() > NAME_MAX {
        return bad("名字最长 64 个字");
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err(Error::Plain(format!("名字里不能有 {c}")));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return bad("名字不能以点开头或结尾");
    }
    // Windows 的保留名，做文件夹名会出错。
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit();
    if reserved.contains(&stem.as_str()) || numbered {
        return bad("这个名字 Windows 不能用");
    }
    Ok(name.to_string())
}

/// 把随便什么名字（比如整合包里写的）变成能用的：不能用的字符换成 `_`，太长截断。
pub fn safe_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(NAME_MAX)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    match check_name(&cleaned) {
        Ok(name) => name,
        Err(_) if cleaned.is_empty() => "整合包".to_string(),
        Err(_) => format!("{cleaned}_"),
    }
}

/// 从 profile 认出 MC 版本、加载器和加载器版本。先看启动参数和库，再看 id 的写法。
/// 用在整合包（加载器由整合包决定）和别的启动器装的版本上。
pub fn detect(root: &Root, profile: &str) -> (String, Loader, Option<String>) {
    let (merged, chain) = match manifest::merged_chain(root, profile) {
        Ok(found) => found,
        Err(_) => return detect_from_id(profile, profile),
    };
    let args: Vec<String> = merged
        .arguments
        .as_ref()
        .map(|a| {
            a.game
                .iter()
                .filter_map(|arg| match arg {
                    manifest::Argument::Plain(s) => Some(s.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let arg = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let library = |prefix: &str| {
        merged
            .libraries
            .iter()
            .find_map(|lib| lib.name.strip_prefix(prefix))
            .map(|rest| rest.split(':').next().unwrap_or(rest).to_string())
    };
    let base = if chain.len() > 1 {
        chain.first().cloned()
    } else {
        None
    };
    let mc = arg("--fml.mcVersion")
        .or(base)
        .unwrap_or_else(|| detect_from_id(profile, profile).0);
    if let Some(version) = library("net.fabricmc:fabric-loader:") {
        return (mc, Loader::Fabric, Some(version));
    }
    if let Some(version) = arg("--fml.neoForgeVersion") {
        return (mc, Loader::NeoForge, Some(version));
    }
    if let Some(version) = arg("--fml.forgeVersion") {
        // 1.20.1 的 NeoForge 用的是 Forge 的参数和坐标。
        let loader = if library("net.neoforged:").is_some() {
            Loader::NeoForge
        } else {
            Loader::Forge
        };
        return (mc, loader, Some(version));
    }
    if let Some(full) = library("net.minecraftforge:forge:") {
        let version = full
            .strip_prefix(&format!("{mc}-"))
            .unwrap_or(&full)
            .to_string();
        return (mc, Loader::Forge, Some(version));
    }
    let (id_mc, loader, version) = detect_from_id(profile, &mc);
    if loader == Loader::Vanilla {
        (mc, loader, version)
    } else {
        (id_mc, loader, version)
    }
}

/// 按加载器 profile 的 id 写法猜：`fabric-loader-0.16.10-1.21.1`、`neoforge-21.1.209`、
/// `1.20.1-forge-47.3.0`、`forge-…`。猜不出就当原版。
fn detect_from_id(id: &str, mc: &str) -> (String, Loader, Option<String>) {
    if let Some(rest) = id.strip_prefix("fabric-loader-") {
        return match rest.rsplit_once('-') {
            Some((version, mc)) => (mc.to_string(), Loader::Fabric, Some(version.to_string())),
            None => (mc.to_string(), Loader::Fabric, Some(rest.to_string())),
        };
    }
    if let Some(rest) = id.strip_prefix("neoforge-") {
        return (mc.to_string(), Loader::NeoForge, Some(rest.to_string()));
    }
    if let Some((id_mc, version)) = id.split_once("-forge-") {
        return (id_mc.to_string(), Loader::Forge, Some(version.to_string()));
    }
    if let Some(rest) = id.strip_prefix("forge-") {
        return match rest.split_once('-') {
            Some((mc, version)) => (mc.to_string(), Loader::Forge, Some(version.to_string())),
            None => (mc.to_string(), Loader::Forge, Some(rest.to_string())),
        };
    }
    (mc.to_string(), Loader::Vanilla, None)
}

/// 默认名：原版是 `1.21.1`，加载器是 `1.21.1 NeoForge`。
pub fn default_name(mc: &str, loader: Loader) -> String {
    match loader {
        Loader::Vanilla => mc.to_string(),
        _ => format!("{mc} {}", loader.label()),
    }
}

/// 没人用过的名字：重名时在后面加数字，`1.21.1 NeoForge 2`。
pub fn unique_name(root: &Root, base: &str) -> String {
    let base = check_name(base).unwrap_or_else(|_| "Minecraft".to_string());
    let taken = |name: &str| {
        let dir = root.version_dir(name);
        dir.exists() || exists_ignoring_case(&root.versions(), name)
    };
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .unwrap_or(base)
}

/// Windows 和 macOS 的文件系统不分大小写：`abc` 和 `ABC` 是同一个文件夹。
fn exists_ignoring_case(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
    })
}

/// 装一个版本：先在缓存里装原版和加载器，再合并写进 `versions/<名字>/`。
/// 名字已经存在时就是补全或换加载器版本：游戏文件不动，只重写 JSON 和 jar。
pub fn install(
    root: &Root,
    name: &str,
    mc: &str,
    loader: Loader,
    loader_version: Option<&str>,
    mirror: &str,
    sink: &Sink,
) -> Result<String> {
    let name = check_name(name)?;
    let store = root.store();
    let version = || {
        loader_version
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::Plain(format!("没有选 {} 的版本", loader.label())))
    };
    let profile = match loader {
        Loader::Vanilla => {
            vanilla::install_with(&store, mc, mirror, sink)?;
            mc.to_string()
        }
        Loader::Fabric => fabric::install_with(&store, mc, version()?, mirror, sink)?,
        Loader::Forge => forge::install_with(&store, mc, version()?, mirror, sink)?,
        Loader::NeoForge => neoforge::install_with(&store, mc, version()?, mirror, sink)?,
    };
    let meta = Meta {
        mc: mc.to_string(),
        loader,
        loader_version: loader_version
            .filter(|_| loader != Loader::Vanilla)
            .map(str::to_string),
        profile: profile.clone(),
        // 补全整合包时，整合包的来历照旧。
        pack: self::meta(root, &name).and_then(|old| old.pack),
    };
    create(root, &name, &profile, meta)?;
    Ok(name)
}

/// 把缓存里的 `profile` 合并成完整的版本 JSON，写进 `versions/<名字>/`，并放好客户端 jar。
pub fn create(root: &Root, name: &str, profile: &str, meta: Meta) -> Result<()> {
    let name = check_name(name)?;
    let store = root.store();
    let resolved = manifest::resolve(&store, profile, &Context::current())?;
    let (mut version, _) = manifest::merged_chain(&store, profile)?;
    version.id = name.clone();
    version.inherits_from = None;
    version.jar = None;
    version.extra.insert(
        META_KEY.to_string(),
        serde_json::to_value(&meta).map_err(|e| Error::Plain(e.to_string()))?,
    );
    let dir = root.version_dir(&name);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    link_or_copy(&resolved.client_jar, &root.client_jar(&name))?;
    write_atomic(&root.version_json(&name), &version_text(&version)?)
}

/// 版本 JSON 的文本。去掉值为空的顶层字段（`"inheritsFrom": null` 之类），别的启动器读着更放心。
fn version_text(version: &VersionJson) -> Result<Vec<u8>> {
    let mut value = serde_json::to_value(version).map_err(|e| Error::Plain(e.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, v| !v.is_null());
    }
    serde_json::to_vec_pretty(&value).map_err(|e| Error::Plain(e.to_string()))
}

/// 硬链接过去，不行就复制；先放到临时文件再改名，不会留下半个 jar。
fn link_or_copy(from: &Path, to: &Path) -> Result<()> {
    let tmp = to.with_extension(format!("jar.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if std::fs::hard_link(from, &tmp).is_err() {
        std::fs::copy(from, &tmp).map_err(|e| io(from, e))?;
    }
    std::fs::rename(&tmp, to).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(to, e)
    })
}

/// 删掉一个版本：整个文件夹移到回收站，包括存档。这个版本的启动参数也去掉。
pub fn remove(root: &Root, name: &str) -> Result<()> {
    let dir = child_dir(root, name)?;
    crate::paths::to_trash(&dir)?;
    forget(root, name);
    Ok(())
}

/// 直接删掉，找不回来。只在回收站用不了、玩家又确认过时用。
pub fn remove_permanently(root: &Root, name: &str) -> Result<()> {
    let dir = child_dir(root, name)?;
    std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
    forget(root, name);
    Ok(())
}

/// 版本没了以后，它的 natives 和启动参数也不留。
fn forget(root: &Root, name: &str) {
    let _ = std::fs::remove_dir_all(root.natives(name));
    let _ = crate::launchcfg::save(root, name, &crate::launchcfg::VersionLaunch::default());
}

/// 改名：文件夹、JSON、jar、JSON 里的 `id` 和启动参数一起改。
pub fn rename(root: &Root, from: &str, to: &str) -> Result<String> {
    let to = check_name(to)?;
    let from_dir = child_dir(root, from)?;
    if to == from {
        return Ok(to);
    }
    let to_dir = root.version_dir(&to);
    // 只改大小写时，不分大小写的文件系统会说「已经有了」：那就是它自己。
    let same_folder = to.eq_ignore_ascii_case(from);
    if !same_folder && (to_dir.exists() || exists_ignoring_case(&root.versions(), &to)) {
        return Err(Error::Plain(format!("已经有叫「{to}」的版本了")));
    }
    let mut version = manifest::load(root, from)?;
    std::fs::rename(&from_dir, &to_dir).map_err(|e| io(&from_dir, e))?;
    let renames = [
        (format!("{from}.json"), format!("{to}.json")),
        (format!("{from}.jar"), format!("{to}.jar")),
    ];
    for (old, new) in renames {
        let old = to_dir.join(old);
        if old.exists() {
            std::fs::rename(&old, to_dir.join(new)).map_err(|e| io(&old, e))?;
        }
    }
    version.id = to.clone();
    write_atomic(&root.version_json(&to), &version_text(&version)?)?;
    let mut all = crate::launchcfg::load_all(root);
    if let Some(settings) = all.remove(from) {
        crate::launchcfg::save(root, &to, &settings)?;
        crate::launchcfg::save(root, from, &crate::launchcfg::VersionLaunch::default())?;
    }
    let _ = std::fs::remove_dir_all(root.natives(from));
    Ok(to)
}

/// `versions/` 下名为 `name` 的文件夹；名字不合法或不存在时报错，绝不会跑出 `versions/`。
fn child_dir(root: &Root, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(Error::Plain(format!("不认识的版本名：{name}")));
    }
    let dir = root.version_dir(name);
    if !dir.is_dir() {
        return Err(Error::Plain(format!("找不到版本「{name}」")));
    }
    Ok(dir)
}

/// 全部版本的名字（`versions/` 下有同名 JSON 的文件夹）。
pub fn names(root: &Root) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.versions()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| root.version_json(name).is_file())
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------- 清理

/// processor 生成的文件（补丁后的客户端、srg、映射）放在这些目录下，
/// 不写在版本 JSON 的库列表里，所以清理时一律保留。
const KEEP_LIBRARIES: [&str; 4] = [
    "net/minecraft/",
    "net/minecraftforge/",
    "net/neoforged/",
    "de/oceanlabs/",
];

/// 清理能删掉的文件。先算好，玩家点了再删。
#[derive(Debug, Clone, Default)]
pub struct Cleanup {
    pub files: Vec<PathBuf>,
    /// 删掉能空出来的字节数（同一份数据的多个硬链接只算一次，还有别处链接着的不算）。
    pub bytes: u64,
}

/// 算出哪些文件已经没有版本在用：缓存里的 profile、资源文件、资源索引、运行库，以及内容库里
/// 没人链接的文件。
pub fn plan_cleanup(root: &Root) -> Result<Cleanup> {
    let ctx = Context::current();
    let store = root.store();
    let mut keep_libraries: HashSet<PathBuf> = HashSet::new();
    let mut keep_indexes: HashSet<String> = HashSet::new();
    let mut keep_profiles: HashSet<String> = HashSet::new();
    let mut keep_sha1: HashSet<String> = HashSet::new();
    let mut collect = |version: &VersionJson| {
        for spec in manifest::library_specs_for(root, &version.libraries, &ctx) {
            if let Some(sha1) = spec.sha1 {
                keep_sha1.insert(sha1.to_ascii_lowercase());
            }
            keep_libraries.insert(spec.dest);
        }
        for lib in &version.libraries {
            if let Ok(coord) = manifest::Coord::parse(&lib.name) {
                keep_libraries.insert(root.libraries().join(coord.rel_path()));
            }
        }
        if let Some(index) = &version.asset_index {
            keep_indexes.insert(index.id.clone());
        }
        if let Some(client) = &version.downloads.client
            && let Some(sha1) = &client.sha1
        {
            keep_sha1.insert(sha1.to_ascii_lowercase());
        }
    };
    for name in names(root) {
        // 读不懂的版本：保守起见什么都不清理。
        let (version, _) = manifest::merged_chain(root, &name)
            .map_err(|e| Error::Plain(format!("版本「{name}」读不懂，先不清理：{e}")))?;
        collect(&version);
        if let Some(meta) = meta(root, &name)
            && let Ok((_, chain)) = manifest::merged_chain(&store, &meta.profile)
        {
            keep_profiles.extend(chain);
        }
    }
    for profile in &keep_profiles {
        if let Ok(version) = manifest::load(&store, profile) {
            collect(&version);
        }
    }

    let mut files = Vec::new();
    // 缓存里没人用的 profile：整个文件夹。
    for entry in read_dir(&root.profiles()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() && !keep_profiles.contains(&name) {
            files.extend(walk_files(&entry.path()));
        }
    }
    // 资源索引和资源文件。
    let mut keep_objects: HashSet<String> = HashSet::new();
    for entry in read_dir(&root.indexes()) {
        let path = entry.path();
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if keep_indexes.contains(&id) {
            let index: Value = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| Error::Plain(format!("资源索引读不懂，先不清理：{id}")))?;
            if let Some(objects) = index.get("objects").and_then(Value::as_object) {
                keep_objects.extend(
                    objects
                        .values()
                        .filter_map(|o| o.get("hash").and_then(Value::as_str))
                        .map(str::to_ascii_lowercase),
                );
            }
        } else if path.is_file() {
            files.push(path);
        }
    }
    for path in walk_files(&root.objects()) {
        let hash = path
            .file_name()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !keep_objects.contains(&hash) {
            files.push(path);
        }
    }
    // 运行库。
    let libraries = root.libraries();
    for path in walk_files(&libraries) {
        let relative = path
            .strip_prefix(&libraries)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if KEEP_LIBRARIES.iter().any(|keep| relative.starts_with(keep)) {
            continue;
        }
        if !keep_libraries.contains(&path) {
            files.push(path);
        }
    }
    // 内容库：别处都没链接它了（Unix 上看链接数），或者没有版本要这个 SHA1（Windows）。
    let mut blobs = Vec::new();
    for path in walk_files(&crate::blob::blob_dir(root)) {
        let sha1 = path
            .file_name()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if sha1.contains('.') {
            // 下载到一半留下的临时文件。
            blobs.push(path);
            continue;
        }
        if blob_unused(&path, &sha1, &keep_sha1) {
            blobs.push(path);
        }
    }
    files.extend(blobs);
    files.sort();
    files.dedup();
    let bytes = freed_bytes(&files);
    Ok(Cleanup { files, bytes })
}

#[cfg(unix)]
fn blob_unused(path: &Path, _sha1: &str, _keep: &HashSet<String>) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|m| m.nlink() <= 1)
}

#[cfg(not(unix))]
fn blob_unused(_path: &Path, sha1: &str, keep: &HashSet<String>) -> bool {
    !keep.contains(sha1)
}

/// 删掉这些文件能空出多少：同一份数据的硬链接只算一次；还有不删的链接时不算。
#[cfg(unix)]
fn freed_bytes(files: &[PathBuf]) -> u64 {
    use std::collections::HashMap;
    use std::os::unix::fs::MetadataExt;
    let mut inodes: HashMap<(u64, u64), (u64, u64, u64)> = HashMap::new();
    for path in files {
        if let Ok(m) = std::fs::symlink_metadata(path) {
            let entry = inodes
                .entry((m.dev(), m.ino()))
                .or_insert((m.len(), m.nlink(), 0));
            entry.2 += 1;
        }
    }
    inodes
        .values()
        .filter(|(_, links, deleting)| deleting >= links)
        .map(|(len, _, _)| len)
        .sum()
}

#[cfg(not(unix))]
fn freed_bytes(files: &[PathBuf]) -> u64 {
    files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

/// 删掉清理计划里的文件，再去掉删空的文件夹。返回删了几个文件。
pub fn run_cleanup(root: &Root, plan: &Cleanup) -> Result<usize> {
    let mut removed = 0;
    for path in &plan.files {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    for dir in [
        root.profiles(),
        root.objects(),
        root.libraries(),
        crate::blob::blob_dir(root),
    ] {
        remove_empty_dirs(&dir);
    }
    Ok(removed)
}

fn read_dir(dir: &Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().collect())
        .unwrap_or_default()
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in read_dir(&dir) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else {
                out.push(entry.path());
            }
        }
    }
    out
}

/// 去掉 `dir` 下面空了的文件夹（`dir` 本身留着）。
fn remove_empty_dirs(dir: &Path) {
    for entry in read_dir(dir) {
        if entry.file_type().is_ok_and(|k| k.is_dir()) {
            let path = entry.path();
            remove_empty_dirs(&path);
            let _ = std::fs::remove_dir(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!(
            "cairn-versions-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Root::new(dir)
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// 缓存里放一个原版和一个继承它的加载器 profile，像安装后那样。
    fn fake_store(root: &Root) {
        let store = root.store();
        write(
            &store.version_json("1.21.1"),
            serde_json::json!({
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {"id": "17", "url": "https://x/17.json"},
                "downloads": {"client": {"url": "https://x/client.jar", "sha1": "abc", "size": 4}},
                "libraries": [
                    {"name": "org.lwjgl:lwjgl:3.3.3",
                     "downloads": {"artifact": {"path": "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar",
                                                "url": "https://x/lwjgl.jar", "sha1": "111"}}}
                ],
                "arguments": {"game": ["--username", "${auth_player_name}"], "jvm": ["-cp", "${classpath}"]}
            })
            .to_string()
            .as_bytes(),
        );
        write(&store.client_jar("1.21.1"), b"jar!");
        write(
            &store.version_json("neoforge-21.1.209"),
            serde_json::json!({
                "id": "neoforge-21.1.209",
                "inheritsFrom": "1.21.1",
                "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "libraries": [
                    {"name": "net.neoforged.fancymodloader:loader:4.0.24",
                     "downloads": {"artifact": {"path": "net/neoforged/fancymodloader/loader/4.0.24/loader-4.0.24.jar",
                                                "url": "https://x/loader.jar", "sha1": "222"}}}
                ],
                "arguments": {"game": ["--fml.neoForgeVersion", "21.1.209"]}
            })
            .to_string()
            .as_bytes(),
        );
    }

    fn neoforge_meta() -> Meta {
        Meta {
            mc: "1.21.1".into(),
            loader: Loader::NeoForge,
            loader_version: Some("21.1.209".into()),
            profile: "neoforge-21.1.209".into(),
            pack: None,
        }
    }

    #[test]
    fn names_are_checked_and_made_unique() {
        assert_eq!(check_name("  生存 1.21.1 ").unwrap(), "生存 1.21.1");
        for bad in [
            "", " ", ".", "..", "a/b", "a\\b", "a:b", "what?", ".hidden", "end.", "CON", "com1",
            "a\u{7}",
        ] {
            assert!(check_name(bad).is_err(), "{bad:?} 应该不能用");
        }
        assert!(check_name(&"字".repeat(64)).is_ok());
        assert!(check_name(&"字".repeat(65)).is_err());
        assert_eq!(default_name("1.21.1", Loader::Vanilla), "1.21.1");
        assert_eq!(default_name("1.21.1", Loader::NeoForge), "1.21.1 NeoForge");

        let root = temp_root("names");
        assert_eq!(unique_name(&root, "1.21.1 NeoForge"), "1.21.1 NeoForge");
        std::fs::create_dir_all(root.version_dir("1.21.1 NeoForge")).unwrap();
        assert_eq!(unique_name(&root, "1.21.1 NeoForge"), "1.21.1 NeoForge 2");
        std::fs::create_dir_all(root.version_dir("1.21.1 neoforge 2")).unwrap();
        assert_eq!(unique_name(&root, "1.21.1 NeoForge"), "1.21.1 NeoForge 3");
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn pack_names_are_made_usable() {
        assert_eq!(safe_name("城市: 启航 / 2"), "城市_ 启航 _ 2");
        assert_eq!(safe_name("  "), "整合包");
        assert_eq!(safe_name("..."), "整合包");
        assert_eq!(safe_name("CON"), "CON_");
        assert_eq!(safe_name(&"字".repeat(80)).chars().count(), 64);
    }

    #[test]
    fn loaders_are_recognized_from_the_profile() {
        let root = temp_root("detect");
        fake_store(&root);
        let store = root.store();
        assert_eq!(
            detect(&store, "neoforge-21.1.209"),
            ("1.21.1".into(), Loader::NeoForge, Some("21.1.209".into()))
        );
        assert_eq!(
            detect(&store, "1.21.1"),
            ("1.21.1".into(), Loader::Vanilla, None)
        );
        // 整合包自带的 JSON：id 看不出来，靠库认出 Fabric。
        write(
            &store.version_json("hmcl-city"),
            br#"{"id":"hmcl-city","inheritsFrom":"1.21.1",
                "libraries":[{"name":"net.fabricmc:fabric-loader:0.16.10"}]}"#,
        );
        assert_eq!(
            detect(&store, "hmcl-city"),
            ("1.21.1".into(), Loader::Fabric, Some("0.16.10".into()))
        );
        assert_eq!(
            detect_from_id("1.20.1-forge-47.3.0", "?"),
            ("1.20.1".into(), Loader::Forge, Some("47.3.0".into()))
        );
        assert_eq!(
            detect_from_id("fabric-loader-0.16.10-1.21.1", "?"),
            ("1.21.1".into(), Loader::Fabric, Some("0.16.10".into()))
        );
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn a_version_folder_holds_a_complete_json_and_its_own_jar() {
        let root = temp_root("create");
        fake_store(&root);
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();

        let text = std::fs::read_to_string(root.version_json("生存")).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["id"], "生存");
        assert!(value.get("inheritsFrom").is_none());
        assert!(value.get("jar").is_none());
        assert_eq!(
            value["mainClass"],
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );
        assert_eq!(value["libraries"].as_array().unwrap().len(), 2);
        assert_eq!(value["assetIndex"]["id"], "17");
        assert_eq!(std::fs::read(root.client_jar("生存")).unwrap(), b"jar!");
        assert_eq!(meta(&root, "生存"), Some(neoforge_meta()));

        // 不依赖缓存：缓存删掉照样能解析。
        std::fs::remove_dir_all(root.profiles()).unwrap();
        let resolved = manifest::resolve(&root, "生存", &Context::current()).unwrap();
        assert_eq!(resolved.chain, vec!["生存".to_string()]);
        assert_eq!(resolved.client_jar, root.client_jar("生存"));
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn reinstalling_keeps_the_game_files() {
        let root = temp_root("repair");
        fake_store(&root);
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();
        let world = root.version_dir("生存").join("saves/家/level.dat");
        write(&world, b"world");
        std::fs::remove_file(root.client_jar("生存")).unwrap();
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();
        assert_eq!(std::fs::read(&world).unwrap(), b"world");
        assert!(root.client_jar("生存").is_file());
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn two_versions_with_the_same_loader_are_independent() {
        let root = temp_root("twins");
        fake_store(&root);
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();
        create(&root, "测试", "neoforge-21.1.209", neoforge_meta()).unwrap();
        write(&root.version_dir("测试").join("mods/a.jar"), b"mod");
        assert!(!root.version_dir("生存").join("mods").exists());
        assert_ne!(root.natives("生存"), root.natives("测试"));
        assert_eq!(names(&root), vec!["测试".to_string(), "生存".to_string()]);
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn renaming_moves_everything_along() {
        let root = temp_root("rename");
        fake_store(&root);
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();
        write(
            &root.version_dir("生存").join("saves/家/level.dat"),
            b"world",
        );
        let settings = crate::launchcfg::VersionLaunch {
            window: Some([1920, 1080]),
            ..Default::default()
        };
        crate::launchcfg::save(&root, "生存", &settings).unwrap();
        create(&root, "测试", "neoforge-21.1.209", neoforge_meta()).unwrap();

        assert!(rename(&root, "生存", "测试").is_err());
        assert!(rename(&root, "生存", "a/b").is_err());
        assert_eq!(rename(&root, "生存", "家园").unwrap(), "家园");
        assert!(!root.version_dir("生存").exists());
        assert_eq!(
            std::fs::read(root.version_dir("家园").join("saves/家/level.dat")).unwrap(),
            b"world"
        );
        assert!(root.client_jar("家园").is_file());
        assert_eq!(manifest::load(&root, "家园").unwrap().id, "家园");
        let all = crate::launchcfg::load_all(&root);
        assert_eq!(all.get("家园"), Some(&settings));
        assert!(!all.contains_key("生存"));
        let _ = std::fs::remove_dir_all(root.dir());
    }

    #[test]
    fn cleanup_only_takes_what_no_version_uses() {
        let root = temp_root("cleanup");
        fake_store(&root);
        let store = root.store();
        // 另一个没人用的 profile。
        write(
            &store.version_json("1.20.1"),
            br#"{"id":"1.20.1","libraries":[]}"#,
        );
        create(&root, "生存", "neoforge-21.1.209", neoforge_meta()).unwrap();

        let libs = root.libraries();
        let used = libs.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar");
        let loader = libs.join("net/neoforged/fancymodloader/loader/4.0.24/loader-4.0.24.jar");
        let old = libs.join("org/ow2/asm/asm/9.0/asm-9.0.jar");
        let patched = libs.join("net/neoforged/neoforge/21.1.209/neoforge-21.1.209-client.jar");
        let srg = libs.join("net/minecraft/client/1.21.1-1/client-1.21.1-1-srg.jar");
        for path in [&used, &loader, &old, &patched, &srg] {
            write(path, b"lib");
        }
        let used_hash = "aa11";
        write(
            &root.indexes().join("17.json"),
            format!(r#"{{"objects":{{"a":{{"hash":"{used_hash}","size":1}}}}}}"#).as_bytes(),
        );
        write(&root.indexes().join("5.json"), br#"{"objects":{}}"#);
        let used_object = root.objects().join("aa").join(used_hash);
        let old_object = root.objects().join("bb").join("bb22");
        write(&used_object, b"o");
        write(&old_object, b"o");

        let plan = plan_cleanup(&root).unwrap();
        let has = |path: &Path| plan.files.iter().any(|p| p == path);
        assert!(has(&old));
        assert!(has(&old_object));
        assert!(has(&root.indexes().join("5.json")));
        assert!(has(&store.version_json("1.20.1")));
        for kept in [&used, &loader, &patched, &srg, &used_object] {
            assert!(!has(kept), "{} 不该删", kept.display());
        }
        assert!(!has(&root.indexes().join("17.json")));
        assert!(!has(&store.version_json("neoforge-21.1.209")));
        assert!(!has(&store.version_json("1.21.1")));
        assert!(
            !plan
                .files
                .iter()
                .any(|p| p.starts_with(root.version_dir("生存")))
        );
        assert!(plan.bytes > 0);

        run_cleanup(&root, &plan).unwrap();
        assert!(!old.exists() && !old_object.exists());
        assert!(!store.version_dir("1.20.1").exists());
        assert!(used.exists() && patched.exists());
        let _ = std::fs::remove_dir_all(root.dir());
    }
}
