//! 启动器自己的更新：查 GitHub 上的最新版本，下载这个系统用的文件，按 SHA256SUMS 校验。
//!
//! - Windows：在后台下载新的 `cairn.exe`，校验后记在 `update/staged.json`。关掉 Cairn 时
//!   （或者下次打开时）把正在运行的程序改名成 `cairn.old.exe`（Windows 允许给运行中的 exe
//!   改名），新程序放到原位置，下次启动时删掉旧的。
//! - Linux：deb 装的会带上 Cairn 的 apt 源，系统每天自动更新。想马上更新时，下载 deb，
//!   用 pkexec 弹出系统的授权窗口装上；装不上就给出一条能直接复制的命令。

use crate::error::{Error, Result, io};
use crate::http::{FileSpec, Http};
use crate::progress::Sink;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const REPO: &str = "dkjsiogu/cairn";

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// 不带 v，例如 `0.38.0`。
    pub version: String,
    /// 发布说明（Markdown 原文）。
    pub notes: String,
    /// 发布页面。
    pub page: String,
    pub assets: Vec<Asset>,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse_release(value: &Value) -> Option<Release> {
    let tag = value.get("tag_name")?.as_str()?;
    let assets = value
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(|asset| {
            Some(Asset {
                name: asset.get("name")?.as_str()?.to_string(),
                url: asset.get("browser_download_url")?.as_str()?.to_string(),
                size: asset.get("size")?.as_u64()?,
            })
        })
        .collect();
    Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: value
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or_default()
            .to_string(),
        page: value
            .get("html_url")
            .and_then(|u| u.as_str())
            .unwrap_or_default()
            .to_string(),
        assets,
    })
}

/// GitHub 上标为最新的正式版。
pub fn latest(http: &Http) -> Result<Release> {
    let value = http.get_json_with_headers(
        &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        &[("Accept", "application/vnd.github+json")],
    )?;
    parse_release(&value).ok_or_else(|| Error::Install("读不懂 GitHub 返回的版本信息".into()))
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

/// `latest` 比 `current` 新。只比较数字部分，`0.38.0` > `0.37.2`。
pub fn is_newer(latest: &str, current: &str) -> bool {
    numbers(latest) > numbers(current)
}

/// 这个系统该下哪个文件。
pub fn asset_for_this_system(release: &Release) -> Option<&Asset> {
    let wanted = |name: &str| {
        if cfg!(windows) {
            name.eq_ignore_ascii_case("cairn.exe")
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            name.starts_with("cairn_") && name.ends_with("_amd64.deb")
        } else {
            false
        }
    };
    release.assets.iter().find(|asset| wanted(&asset.name))
}

fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| io(path, e))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| io(path, e))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(crate::http::hex(&hash.finalize()))
}

/// SHA256SUMS 里某个文件的值（`<hash>  <name>` 或 `<hash>  ./<name>`）。
fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*').trim_start_matches("./");
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

/// 下载到 `dir`，按发布里的 SHA256SUMS 校验，返回文件路径。
pub fn download(
    http: &Http,
    release: &Release,
    asset: &Asset,
    dir: &Path,
    sink: &Sink,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .ok_or_else(|| Error::Install("这个版本没有发布校验值，不安全，没有下载".into()))?;
    let expected = expected_sha256(&http.get_text(&sums.url)?, &asset.name)
        .ok_or_else(|| Error::Install(format!("校验值里没有 {}", asset.name)))?;
    let dest = dir.join(&asset.name);
    let _ = std::fs::remove_file(&dest);
    http.download_with(
        FileSpec {
            url: asset.url.clone(),
            dest: dest.clone(),
            sha1: None,
            size: Some(asset.size),
            verify_existing: false,
            label: Some(asset.name.clone()),
        },
        &format!("下载 Cairn {}", release.version),
        sink,
    )?;
    let actual = sha256_file(&dest)?;
    if actual != expected {
        let _ = std::fs::remove_file(&dest);
        return Err(Error::Install(
            "下载的文件和发布的校验值对不上，已删除；请稍后重试".into(),
        ));
    }
    Ok(dest)
}

/// Cairn 是怎么装的，决定怎么更新。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Install {
    /// Windows 上的单个 exe：后台下载，关掉 Cairn 时换成新版本。
    Exe,
    /// deb 装的，加了 Cairn 的 apt 源：系统每天自动更新，也能在启动器里马上更新。
    AptRepo,
    /// deb 装的，没有 apt 源：在启动器里更新，要输一次密码。
    Deb,
    /// 自己编译的或者别的装法：只提示新版本。
    #[default]
    Other,
}

impl Install {
    /// 能不能由 Cairn 自己（或系统）装好新版本。
    pub fn can_install(self) -> bool {
        !matches!(self, Install::Other)
    }
}

/// deb 装到哪。
pub const DEB_BIN: &str = "/usr/bin/cairn";
/// deb 写的 apt 源。
pub const APT_SOURCE: &str = "/etc/apt/sources.list.d/cairn.sources";
/// apt 源的地址，用来认出玩家自己加的源。
pub const APT_URL: &str = "https://dkjsiogu.github.io/cairn/apt";

/// 正在运行的程序在哪。Linux 上程序被替换后，`/proc/self/exe` 末尾会多出 ` (deleted)`。
fn running_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let text = exe.to_string_lossy();
    Some(match text.strip_suffix(" (deleted)") {
        Some(path) => PathBuf::from(path),
        None => exe,
    })
}

/// 这次运行的 Cairn 是怎么装的。
pub fn install_kind() -> Install {
    if cfg!(windows) {
        return Install::Exe;
    }
    if !cfg!(target_os = "linux") {
        return Install::Other;
    }
    let from_deb = running_exe().as_deref() == Some(Path::new(DEB_BIN))
        && Path::new("/var/lib/dpkg/info/cairn.list").exists();
    if !from_deb {
        Install::Other
    } else if has_apt_source(Path::new("/etc/apt")) {
        Install::AptRepo
    } else {
        Install::Deb
    }
}

/// `etc` 下有没有 Cairn 的 apt 源：deb 写的那个，或者玩家自己加的同一个地址。
fn has_apt_source(etc: &Path) -> bool {
    let mut files = vec![etc.join("sources.list")];
    if let Ok(dir) = std::fs::read_dir(etc.join("sources.list.d")) {
        files.extend(dir.flatten().map(|entry| entry.path()));
    }
    files.iter().any(|file| {
        std::fs::read_to_string(file).is_ok_and(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.starts_with('#'))
                .any(|line| line.contains(APT_URL))
        })
    })
}

/// Linux：正在运行的程序已经被新版本换掉了（系统在后台更新过）。重启 Cairn 就是新版本。
pub fn replaced_on_disk() -> bool {
    cfg!(target_os = "linux")
        && std::fs::read_link("/proc/self/exe")
            .is_ok_and(|exe| exe.to_string_lossy().ends_with(" (deleted)"))
}

/// Linux：dpkg 里记的 Cairn 版本。
pub fn installed_version() -> Option<String> {
    let out = std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", "cairn"])
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !version.is_empty()).then_some(version)
}

/// Linux：用 pkexec 弹出系统的授权窗口，装下载好的 deb。会一直等到玩家输完密码、装完。
pub fn install_deb(deb: &Path) -> Result<()> {
    let out = std::process::Command::new("pkexec")
        .args(["/usr/bin/apt-get", "install", "-y"])
        .arg(deb)
        .output()
        .map_err(|e| Error::Install(format!("没能运行 pkexec：{e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(Error::Install(match out.status.code() {
        Some(126) => "没有授权".into(),
        Some(127) => "没能弹出授权窗口".into(),
        _ => apt_error(&stderr),
    }))
}

/// apt-get 失败时的说明：取 `E:` 开头的那几行。
fn apt_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("E:"))
        .map(|line| line.trim_start_matches("E:").trim())
        .collect();
    if lines.iter().any(|line| line.contains("lock")) {
        return "系统正在安装别的软件，稍后再试".into();
    }
    match lines.last() {
        Some(line) => format!("apt 没装上：{line}"),
        None => "apt 没装上".into(),
    }
}

/// 上次更新留下的旧程序。
fn old_exe(current: &Path) -> PathBuf {
    current.with_file_name("cairn.old.exe")
}

/// 启动时调用：删掉上次更新留下的旧程序，和已经用不上的下载。
pub fn cleanup(cache: &Path) {
    if let Some(current) = running_exe() {
        let _ = std::fs::remove_file(old_exe(&current));
    }
    if cfg!(windows) && staged(cache).is_none() {
        let _ = std::fs::remove_file(staged_path(cache));
        let _ = std::fs::remove_file(cache.join("update").join("cairn.exe"));
    }
}

/// 重新打开 Cairn（用磁盘上的程序，也就是新版本），参数照旧。调用方随后应退出。
pub fn relaunch() -> Result<()> {
    let exe = running_exe().ok_or_else(|| Error::Install("找不到正在运行的程序".into()))?;
    std::process::Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map_err(|e| Error::Install(format!("没能重新打开：{e}")))?;
    Ok(())
}

/// 某个版本的发布页面。
pub fn release_page(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{version}")
}

/// Windows：程序所在的文件夹能不能写。不能写就没法自己换成新版本。
pub fn can_replace_exe() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let Some(dir) = running_exe().and_then(|exe| exe.parent().map(Path::to_path_buf)) else {
        return false;
    };
    let probe = dir.join(".cairn-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Windows：用新程序替换正在运行的程序。`relaunch` 时再启动新的，调用方随后应退出。
fn swap_exe(new_exe: &Path, relaunch: bool) -> Result<()> {
    if !cfg!(windows) {
        return Err(Error::Install("只有 Windows 能自动替换程序".into()));
    }
    let current = running_exe().ok_or_else(|| Error::Install("找不到正在运行的程序".into()))?;
    let old = old_exe(&current);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&current, &old).map_err(|e| io(&current, e))?;
    if let Err(e) = std::fs::copy(new_exe, &current) {
        // 放不进去就把旧程序改回来，至少还能用。
        let _ = std::fs::rename(&old, &current);
        return Err(io(&current, e));
    }
    if relaunch {
        std::process::Command::new(&current)
            .args(std::env::args_os().skip(1))
            .spawn()
            .map_err(|e| Error::Install(format!("新版本没能启动：{e}")))?;
    }
    Ok(())
}

/// Windows：已经下载、校验好，等着换上的新版本。记在 `update/staged.json`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Staged {
    pub version: String,
    pub file: PathBuf,
    pub sha256: String,
}

fn staged_path(cache: &Path) -> PathBuf {
    cache.join("update").join("staged.json")
}

/// 记下一个下载好的新版本，关掉 Cairn 或下次打开时换上。
pub fn stage(cache: &Path, version: &str, file: &Path) -> Result<Staged> {
    let staged = Staged {
        version: version.to_string(),
        file: file.to_path_buf(),
        sha256: sha256_file(file)?,
    };
    let path = staged_path(cache);
    std::fs::create_dir_all(cache.join("update")).map_err(|e| io(cache, e))?;
    let bytes = serde_json::to_vec_pretty(&staged).map_err(|e| Error::Install(e.to_string()))?;
    std::fs::write(&path, bytes).map_err(|e| io(&path, e))?;
    Ok(staged)
}

/// 等着换上的新版本；比现在的旧或者文件不在了就不算。
pub fn staged(cache: &Path) -> Option<Staged> {
    let bytes = std::fs::read(staged_path(cache)).ok()?;
    let staged: Staged = serde_json::from_slice(&bytes).ok()?;
    (is_newer(&staged.version, current()) && staged.file.is_file()).then_some(staged)
}

/// Windows：换上等着的新版本，返回换上的版本号；没有就返回 None。
/// 换之前再校验一次文件。不管成没成，都不再留着这次的记录，免得每次启动都失败一遍。
pub fn apply_staged(cache: &Path, relaunch: bool) -> Result<Option<String>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let Some(staged) = staged(cache) else {
        let _ = std::fs::remove_file(staged_path(cache));
        return Ok(None);
    };
    let _ = std::fs::remove_file(staged_path(cache));
    if sha256_file(&staged.file)? != staged.sha256 {
        let _ = std::fs::remove_file(&staged.file);
        return Err(Error::Install("下载的新版本被改动过，已删除".into()));
    }
    swap_exe(&staged.file, relaunch)?;
    let _ = std::fs::remove_file(&staged.file);
    Ok(Some(staged.version))
}

/// 下载新版本放在哪：Windows 放数据目录的缓存里（马上就替换），Linux 放「下载」目录（要手动装）。
pub fn download_dir(cache: &Path) -> PathBuf {
    if cfg!(windows) {
        return cache.join("update");
    }
    directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| cache.join("update"))
}

/// Linux 上装 deb 的命令，给玩家复制。
pub fn install_command(deb: &Path) -> String {
    format!("sudo apt install '{}'", deb.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.38.0", "0.37.2"));
        assert!(is_newer("v0.37.10", "0.37.9"));
        assert!(is_newer("1.0.0", "0.99.9"));
        assert!(!is_newer("0.37.0", "0.37.0"));
        assert!(!is_newer("0.36.9", "0.37.0"));
    }

    #[test]
    fn reads_a_github_release() {
        let value = serde_json::json!({
            "tag_name": "v0.38.0",
            "html_url": "https://github.com/dkjsiogu/cairn/releases/tag/v0.38.0",
            "body": "改了很多",
            "assets": [
                {"name": "cairn.exe", "browser_download_url": "https://x/cairn.exe", "size": 9000000},
                {"name": "cairn_0.38.0_amd64.deb", "browser_download_url": "https://x/c.deb", "size": 4000000},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS", "size": 200}
            ]
        });
        let release = parse_release(&value).unwrap();
        assert_eq!(release.version, "0.38.0");
        assert_eq!(release.assets.len(), 3);
        let asset = asset_for_this_system(&release);
        if cfg!(windows) {
            assert_eq!(asset.unwrap().name, "cairn.exe");
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(asset.unwrap().name, "cairn_0.38.0_amd64.deb");
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cairn-update-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_the_apt_source_however_it_was_added() {
        let dir = temp_dir("apt");
        let etc = dir.as_path();
        std::fs::create_dir_all(etc.join("sources.list.d")).unwrap();
        assert!(!has_apt_source(etc));
        // 注释掉的不算。
        std::fs::write(
            etc.join("sources.list.d/other.list"),
            format!("# deb {APT_URL} stable main\n"),
        )
        .unwrap();
        assert!(!has_apt_source(etc));
        // deb 写的 deb822 格式。
        std::fs::write(
            etc.join("sources.list.d/cairn.sources"),
            format!("Types: deb\nURIs: {APT_URL}\nSuites: stable\nComponents: main\n"),
        )
        .unwrap();
        assert!(has_apt_source(etc));
        // 玩家自己加的一行式。
        std::fs::remove_file(etc.join("sources.list.d/cairn.sources")).unwrap();
        std::fs::write(
            etc.join("sources.list"),
            format!("deb [signed-by=/x.gpg] {APT_URL} stable main\n"),
        )
        .unwrap();
        assert!(has_apt_source(etc));
    }

    #[test]
    fn apt_errors_read_plainly() {
        assert_eq!(
            apt_error(
                "E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process 42"
            ),
            "系统正在安装别的软件，稍后再试"
        );
        assert_eq!(
            apt_error(
                "W: something\nE: Unable to correct problems, you have held broken packages.\n"
            ),
            "apt 没装上：Unable to correct problems, you have held broken packages."
        );
        assert_eq!(apt_error(""), "apt 没装上");
    }

    #[test]
    fn a_staged_update_is_only_kept_while_it_is_newer() {
        let dir = temp_dir("staged");
        let cache = dir.as_path();
        std::fs::create_dir_all(cache.join("update")).unwrap();
        let file = cache.join("update/cairn.exe");
        std::fs::write(&file, b"new program").unwrap();
        let staged = stage(cache, "999.0.0", &file).unwrap();
        assert_eq!(super::staged(cache), Some(staged.clone()));
        assert_eq!(staged.sha256.len(), 64);
        // 已经是这个版本或更新的了：不再算。
        stage(cache, current(), &file).unwrap();
        assert_eq!(super::staged(cache), None);
        // 文件没了也不算。
        stage(cache, "999.0.0", &file).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!(super::staged(cache), None);
        if !cfg!(windows) {
            // 别的系统上什么都不换，也不会动正在运行的程序。
            assert_eq!(apply_staged(cache, false).unwrap(), None);
        }
    }

    #[test]
    fn finds_the_checksum_for_a_file() {
        let sums = "ABC123  cairn.exe\ndef456  ./cairn_0.38.0_amd64.deb\n";
        assert_eq!(
            expected_sha256(sums, "cairn.exe").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            expected_sha256(sums, "cairn_0.38.0_amd64.deb").as_deref(),
            Some("def456")
        );
        assert_eq!(expected_sha256(sums, "other"), None);
    }
}
