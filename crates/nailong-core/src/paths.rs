use crate::error::{Error, Result, io};
use std::path::{Path, PathBuf};

/// 数据目录里的程序名。0.37 起叫 Cairn，以前叫 nailong。
const APP_DIR: &str = "cairn";
const LEGACY_APP_DIR: &str = "nailong";

/// 旧版的数据在 `<系统数据目录>/nailong`：新目录还不存在时整个搬过来（同一块盘上只是改名）；
/// 搬不动（例如被占用或跨磁盘）就继续用旧目录，什么都不丢。
fn data_home(base: &Path) -> PathBuf {
    let new = base.join(APP_DIR);
    let old = base.join(LEGACY_APP_DIR);
    if !new.exists() && old.is_dir() {
        return if std::fs::rename(&old, &new).is_ok() {
            new
        } else {
            old
        };
    }
    new
}

#[derive(Debug, Clone)]
pub struct Root {
    dir: PathBuf,
    /// 版本放在哪。正常是 `versions/`；`store()` 返回的视图指向 `cache/cairn/profiles/`，
    /// 安装用的原版和加载器 profile 装在那里，合并后才写进 `versions/<名字>/`。
    versions: Option<PathBuf>,
}

impl Root {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        // 游戏进程的工作目录是版本目录，相对路径会让类路径全部失效。
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        Self {
            dir,
            versions: None,
        }
    }

    /// 安装用的视图：`versions()` 指向 `cache/cairn/profiles/`，其余目录不变。
    /// 安装原版和加载器的代码照旧写 `version_json` 和 `client_jar`，只是落在缓存里。
    pub fn store(&self) -> Root {
        Root {
            dir: self.dir.clone(),
            versions: Some(self.profiles()),
        }
    }

    /// 原版和加载器 profile 的缓存目录。
    pub fn profiles(&self) -> PathBuf {
        self.cache().join("profiles")
    }

    pub fn default_root() -> PathBuf {
        // Windows：默认便携，游戏数据放在程序旁边的 .minecraft，改名不影响。
        #[cfg(windows)]
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            return dir.join(".minecraft");
        }
        directories::BaseDirs::new()
            .map(|d| data_home(d.data_dir()).join(".minecraft"))
            .unwrap_or_else(|| PathBuf::from(".minecraft"))
    }

    /// 0.37 以前缓存在 `cache/nailong`：新目录还没有时直接改名过去。
    pub fn migrate_legacy_cache(&self) {
        let old = self.dir.join("cache").join(LEGACY_APP_DIR);
        let new = self.cache();
        if !new.exists() && old.is_dir() {
            let _ = std::fs::rename(old, new);
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn versions(&self) -> PathBuf {
        self.versions
            .clone()
            .unwrap_or_else(|| self.dir.join("versions"))
    }

    pub fn libraries(&self) -> PathBuf {
        self.dir.join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.dir.join("assets")
    }

    pub fn indexes(&self) -> PathBuf {
        self.assets().join("indexes")
    }

    pub fn objects(&self) -> PathBuf {
        self.assets().join("objects")
    }

    pub fn cache(&self) -> PathBuf {
        self.dir.join("cache").join(APP_DIR)
    }

    /// 启动时解压 native 库的目录。路径只用 ASCII（旧版 LWJGL 读不了别的）；名字里有
    /// 别的字符时加一段哈希，免得「生存」和「测试」都变成 `__` 撞在一起。
    pub fn natives(&self, profile: &str) -> PathBuf {
        let safe = sanitize(profile);
        let name = if safe == profile {
            safe
        } else {
            use sha2::{Digest, Sha256};
            let hash = crate::http::hex(&Sha256::digest(profile.as_bytes()));
            format!("{safe}-{}", &hash[..8])
        };
        self.dir.join("natives").join(name)
    }

    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }

    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.json"))
    }

    pub fn client_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }

    /// Present while a Forge/NeoForge processor chain has not completed.
    /// A partial patched client must never be treated as a healthy install.
    pub fn incomplete_marker(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(".nailong-incomplete")
    }

    /// Remove a version directory by its exact on-disk name.  Only direct
    /// children are accepted, so a profile id can never escape `versions/`.
    pub fn remove_version(&self, id: &str) -> Result<()> {
        remove_child(&self.versions(), id)
    }

    pub fn ensure(&self) -> Result<()> {
        for p in [
            self.dir.clone(),
            self.versions(),
            self.libraries(),
            self.assets(),
            self.indexes(),
            self.objects(),
            self.cache(),
        ] {
            std::fs::create_dir_all(&p).map_err(|e| io(&p, e))?;
        }
        Ok(())
    }
}

fn remove_child(parent: &Path, id: &str) -> Result<()> {
    if id.is_empty() || id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        return Err(crate::error::Error::Install(format!(
            "拒绝删除非法路径: {id}"
        )));
    }
    let path = parent.join(id);
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(crate::error::Error::Install(format!(
            "不是目录，拒绝删除: {}",
            path.display()
        )));
    }
    std::fs::remove_dir_all(&path).map_err(|e| io(&path, e))
}

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(path, e)
    })?;
    Ok(())
}

/// 玩家在设置里选的数据目录，记在 Cairn 自己的配置里（不放进任何一个 .minecraft）。
fn chosen_root_file() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.config_dir().join(APP_DIR).join("data-dir.txt"))
}

/// 玩家选过的数据目录；没选过、或者那个文件夹已经不在了，返回 None（用默认的）。
pub fn chosen_root() -> Option<PathBuf> {
    let text = std::fs::read_to_string(chosen_root_file()?).ok()?;
    let dir = PathBuf::from(text.trim());
    (!text.trim().is_empty() && dir.is_dir()).then_some(dir)
}

/// 记下（`Some`）或忘掉（`None`，回到默认）选的数据目录。下次打开 Cairn 生效。
pub fn choose_root(dir: Option<&Path>) -> Result<()> {
    let Some(file) = chosen_root_file() else {
        return Err(Error::Plain("找不到放设置的地方".into()));
    };
    match dir {
        Some(dir) => write_atomic(&file, dir.to_string_lossy().as_bytes()),
        None => match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(&file, e)),
        },
    }
}

/// 换数据目录时一起带过去的：账户、界面设置、每个版本的启动参数、「测一下这台电脑」的结论。
pub const CARRIED: [&str; 4] = [
    "accounts.json",
    "ui.json",
    "launch-settings.json",
    "bench.json",
];

/// 把 [`CARRIED`] 复制到新的数据目录；那边已经有的不覆盖。返回复制了几个。
pub fn carry_settings(from: &Root, to: &Path) -> Result<usize> {
    std::fs::create_dir_all(to).map_err(|e| io(to, e))?;
    let mut copied = 0;
    for name in CARRIED {
        let source = from.dir().join(name);
        let dest = to.join(name);
        if source.is_file() && !dest.exists() {
            std::fs::copy(&source, &dest).map_err(|e| io(&dest, e))?;
            // 账户里有登录凭证：和原来一样只给自己读写。
            #[cfg(unix)]
            if name == "accounts.json" {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600));
            }
            copied += 1;
        }
    }
    Ok(copied)
}

/// 移到回收站（不是直接删掉）。Linux 用 `gio trash`，Windows 用系统的回收站。
pub fn to_trash(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            SHFILEOPSTRUCTW, SHFileOperationW,
        };
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        // 路径要以两个 \0 结尾。
        let mut from: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        from.extend_from_slice(&[0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as _,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        // SAFETY: 结构体里的指针都指向这里活着的缓冲区，调用是同步的。
        let code = unsafe { SHFileOperationW(&mut op) };
        if code != 0 || op.fAnyOperationsAborted != 0 {
            return Err(Error::Plain(format!("没能移到回收站（错误 {code}）")));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new("gio")
            .arg("trash")
            .arg(path)
            .output();
        match output {
            Ok(out) if out.status.success() => Ok(()),
            // 比如放在 /tmp 或 U 盘上：这个位置没有回收站。
            Ok(_) => Err(Error::Plain("这个位置不能用回收站".into())),
            Err(_) => Err(Error::Plain("没能移到回收站：找不到 gio".into())),
        }
    }
}

#[cfg(test)]
mod root_tests {
    use super::*;

    #[test]
    fn switching_folders_carries_settings_without_overwriting() {
        let base = std::env::temp_dir().join(format!("cairn-carry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let old = Root::new(base.join("old"));
        std::fs::create_dir_all(old.dir()).unwrap();
        std::fs::write(old.dir().join("accounts.json"), b"accounts").unwrap();
        std::fs::write(old.dir().join("ui.json"), b"old ui").unwrap();
        let new = base.join("pcl/.minecraft");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("ui.json"), b"new ui").unwrap();
        assert_eq!(carry_settings(&old, &new).unwrap(), 1);
        assert_eq!(
            std::fs::read(new.join("accounts.json")).unwrap(),
            b"accounts"
        );
        assert_eq!(std::fs::read(new.join("ui.json")).unwrap(), b"new ui");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn moves_the_old_data_dir_once() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("cairn-rename-{}-{n}", std::process::id()));
        std::fs::create_dir_all(base.join("nailong/.minecraft/saves/World")).unwrap();
        std::fs::write(base.join("nailong/.minecraft/ui.json"), b"{}").unwrap();
        assert_eq!(data_home(&base), base.join("cairn"));
        assert!(base.join("cairn/.minecraft/saves/World").is_dir());
        assert!(base.join("cairn/.minecraft/ui.json").is_file());
        assert!(!base.join("nailong").exists());
        // 新目录已经在了：不再动旧目录。
        std::fs::create_dir_all(base.join("nailong/.minecraft")).unwrap();
        assert_eq!(data_home(&base), base.join("cairn"));
        assert!(base.join("nailong").exists());
        let root = Root::new(base.join("cairn/.minecraft"));
        std::fs::create_dir_all(root.dir().join("cache/nailong/logs")).unwrap();
        root.migrate_legacy_cache();
        assert!(root.cache().join("logs").is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn relative_roots_become_absolute() {
        let root = Root::new("relative/.minecraft");
        assert!(root.dir().is_absolute());
        assert!(root.dir().ends_with("relative/.minecraft"));
    }
}
