//! World (save) listing, quick delete, zip backup and restore.
//!
//! Backups live in `<gameDir>/backups` and keep the world folder as the zip
//! root, so they can be restored on another instance by unzipping into
//! `saves/`.

use crate::error::{Error, Result};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Debug, Clone)]
pub struct World {
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

pub fn saves_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("saves")
}

pub fn backups_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("backups")
}

fn safe_name(name: &str) -> Result<&str> {
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name),
        _ => Err(Error::Install(format!("非法的存档名: {name}"))),
    }
}

pub fn worlds(game_dir: &Path) -> Result<Vec<World>> {
    let dir = saves_dir(game_dir);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join("level.dat").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let (size, modified) = dir_stats(&path)?;
        out.push(World {
            name,
            size,
            modified,
        });
    }
    out.sort_by_key(|item| std::cmp::Reverse(item.modified));
    Ok(out)
}

pub fn delete_world(game_dir: &Path, name: &str) -> Result<()> {
    let name = safe_name(name)?;
    let dir = saves_dir(game_dir).join(name);
    if !dir.join("level.dat").is_file() {
        return Err(Error::Install(format!("找不到存档: {name}")));
    }
    fs::remove_dir_all(&dir).map_err(|e| crate::error::io(&dir, e))
}

pub fn backup_world(game_dir: &Path, name: &str) -> Result<PathBuf> {
    let name = safe_name(name)?;
    let source = saves_dir(game_dir).join(name);
    if !source.join("level.dat").is_file() {
        return Err(Error::Install(format!("找不到存档: {name}")));
    }
    let dir = backups_dir(game_dir);
    fs::create_dir_all(&dir).map_err(|e| crate::error::io(&dir, e))?;
    let stamp = timestamp();
    let path = dir.join(format!("{name}-{stamp}.zip"));
    let file = File::create(&path).map_err(|e| crate::error::io(&path, e))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    add_dir(&mut zip, &source, Path::new(name), options)?;
    zip.finish()
        .map_err(|e| Error::Install(format!("备份写入失败: {e}")))?;
    Ok(path)
}

pub fn backups(game_dir: &Path) -> Result<Vec<Backup>> {
    let dir = backups_dir(game_dir);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let meta = entry.metadata().ok();
        out.push(Backup {
            name: entry.file_name().to_string_lossy().to_string(),
            size: meta.as_ref().map(|m| m.len()).unwrap_or_default(),
            modified: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or_default(),
        });
    }
    out.sort_by_key(|item| std::cmp::Reverse(item.modified));
    Ok(out)
}

pub fn delete_backup(game_dir: &Path, name: &str) -> Result<()> {
    let name = safe_name(name)?;
    let path = backups_dir(game_dir).join(name);
    fs::remove_file(&path).map_err(|e| crate::error::io(&path, e))
}

/// 把备份解压回 `saves/<world>`；目标已存在时拒绝，避免覆盖现有存档。
pub fn restore_backup(game_dir: &Path, name: &str) -> Result<String> {
    let name = safe_name(name)?;
    let path = backups_dir(game_dir).join(name);
    let file = File::open(&path).map_err(|e| crate::error::io(&path, e))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| Error::Install(format!("备份包损坏: {e}")))?;
    if archive.is_empty() {
        return Err(Error::Install("备份包是空的".into()));
    }
    let mut world = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| Error::Install(format!("读取备份失败: {e}")))?;
        let Some(inner) = entry.enclosed_name() else {
            return Err(Error::Install("备份包里有越界路径".into()));
        };
        let Some(Component::Normal(first)) = inner.components().next() else {
            return Err(Error::Install("备份包结构异常".into()));
        };
        let first = first.to_string_lossy().to_string();
        match &world {
            Some(existing) if *existing != first => {
                return Err(Error::Install("备份包里包含多个存档".into()));
            }
            None => world = Some(first),
            _ => {}
        }
    }
    let Some(world) = world else {
        return Err(Error::Install("备份包结构异常".into()));
    };
    let target = saves_dir(game_dir).join(&world);
    if target.exists() {
        return Err(Error::Install(format!("存档 {world} 已存在，未覆盖")));
    }
    let saves = saves_dir(game_dir);
    fs::create_dir_all(&saves).map_err(|e| crate::error::io(&saves, e))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| Error::Install(format!("读取备份失败: {e}")))?;
        let Some(inner) = entry.enclosed_name() else {
            return Err(Error::Install("备份包里有越界路径".into()));
        };
        let dest = saves.join(&inner);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| crate::error::io(&dest, e))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| crate::error::io(parent, e))?;
        }
        let mut out = File::create(&dest).map_err(|e| crate::error::io(&dest, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| crate::error::io(&dest, e))?;
        out.flush().ok();
    }
    Ok(world)
}

/// 拖进来的东西是不是一个世界：带 `level.dat` 的文件夹，或者里面有世界的 zip。
pub fn is_world(path: &Path) -> bool {
    if path.is_dir() {
        return path.join("level.dat").is_file();
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        && File::open(path)
            .ok()
            .and_then(|file| ZipArchive::new(file).ok())
            .is_some_and(|mut archive| world_prefix(&mut archive).is_some())
}

/// zip 里世界所在的那一层：`level.dat` 最浅的那个所在的目录（可能就是根）。
fn world_prefix(archive: &mut ZipArchive<File>) -> Option<PathBuf> {
    let mut best: Option<PathBuf> = None;
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(index) else {
            continue;
        };
        let Some(inner) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_dir() || inner.file_name().and_then(|n| n.to_str()) != Some("level.dat") {
            continue;
        }
        let prefix = inner.parent().map(Path::to_path_buf).unwrap_or_default();
        let depth = prefix.components().count();
        if best
            .as_ref()
            .is_none_or(|current| depth < current.components().count())
        {
            best = Some(prefix);
        }
    }
    best
}

/// `saves/` 里没人用的名字：重名时在后面加 ` (2)`。
fn free_world_name(saves: &Path, wanted: &str) -> String {
    let cleaned: String = wanted
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let base = cleaned.trim().trim_matches('.').trim();
    let base = if base.is_empty() { "World" } else { base };
    if !saves.join(base).exists() {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !saves.join(name).exists())
        .unwrap_or_else(|| base.to_string())
}

/// 把一个世界（文件夹或 zip）导入 `saves/`，不覆盖已有的；返回导入后的名字。
pub fn import_world(game_dir: &Path, source: &Path) -> Result<String> {
    let saves = saves_dir(game_dir);
    fs::create_dir_all(&saves).map_err(|e| crate::error::io(&saves, e))?;
    let stem = |path: &Path| {
        path.file_stem()
            .or_else(|| path.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "World".into())
    };
    if source.is_dir() {
        if !source.join("level.dat").is_file() {
            return Err(Error::Install(
                "这个文件夹不是世界：里面没有 level.dat".into(),
            ));
        }
        let wanted = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "World".into());
        let name = free_world_name(&saves, &wanted);
        let part = saves.join(format!(".{name}.part"));
        let _ = fs::remove_dir_all(&part);
        if let Err(e) = copy_dir(source, &part) {
            let _ = fs::remove_dir_all(&part);
            return Err(e);
        }
        let target = saves.join(&name);
        fs::rename(&part, &target).map_err(|e| crate::error::io(&part, e))?;
        return Ok(name);
    }
    let file = File::open(source).map_err(|e| crate::error::io(source, e))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| Error::Install(format!("压缩包打不开: {e}")))?;
    let Some(prefix) = world_prefix(&mut archive) else {
        return Err(Error::Install(
            "压缩包里没有世界（找不到 level.dat）".into(),
        ));
    };
    let wanted = match prefix.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => stem(source),
    };
    let name = free_world_name(&saves, &wanted);
    let part = saves.join(format!(".{name}.part"));
    let _ = fs::remove_dir_all(&part);
    let extract = |archive: &mut ZipArchive<File>| -> Result<()> {
        for index in 0..archive.len() {
            crate::cancel::check()?;
            let mut entry = archive
                .by_index(index)
                .map_err(|e| Error::Install(format!("读取压缩包失败: {e}")))?;
            let Some(inner) = entry.enclosed_name() else {
                return Err(Error::Install("压缩包里有越界路径".into()));
            };
            let Ok(relative) = inner.strip_prefix(&prefix) else {
                continue;
            };
            let dest = part.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&dest).map_err(|e| crate::error::io(&dest, e))?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| crate::error::io(parent, e))?;
            }
            let mut out = File::create(&dest).map_err(|e| crate::error::io(&dest, e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| crate::error::io(&dest, e))?;
        }
        Ok(())
    };
    if let Err(e) = extract(&mut archive) {
        let _ = fs::remove_dir_all(&part);
        return Err(e);
    }
    let target = saves.join(&name);
    fs::rename(&part, &target).map_err(|e| crate::error::io(&part, e))?;
    Ok(name)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| crate::error::io(to, e))?;
    for entry in fs::read_dir(from)
        .map_err(|e| crate::error::io(from, e))?
        .flatten()
    {
        crate::cancel::check()?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            copy_dir(&source, &target)?;
        } else if kind.is_file() {
            // session.lock 是游戏开着时的锁，带过去没用。
            if entry.file_name() == "session.lock" {
                continue;
            }
            fs::copy(&source, &target).map_err(|e| crate::error::io(&source, e))?;
        }
    }
    Ok(())
}

fn add_dir(
    zip: &mut ZipWriter<File>,
    dir: &Path,
    prefix: &Path,
    options: SimpleFileOptions,
) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|e| crate::error::io(dir, e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let inner = prefix.join(&name);
        let meta = entry.metadata().map_err(|e| crate::error::io(&path, e))?;
        if meta.is_dir() {
            let dir_name = format!("{}/", inner.to_string_lossy().replace('\\', "/"));
            zip.add_directory(dir_name, options)
                .map_err(|e| Error::Install(format!("备份写入失败: {e}")))?;
            add_dir(zip, &path, &inner, options)?;
        } else if meta.is_file() {
            let file_name = inner.to_string_lossy().replace('\\', "/");
            zip.start_file(file_name, options)
                .map_err(|e| Error::Install(format!("备份写入失败: {e}")))?;
            let mut file = File::open(&path).map_err(|e| crate::error::io(&path, e))?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|e| crate::error::io(&path, e))?;
            zip.write_all(&buffer)
                .map_err(|e| Error::Install(format!("备份写入失败: {e}")))?;
        }
    }
    Ok(())
}

fn dir_stats(dir: &Path) -> Result<(u64, u64)> {
    let meta = fs::metadata(dir).map_err(|e| crate::error::io(dir, e))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut size = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                size += meta.len();
            }
        }
    }
    Ok((size, modified))
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    // 只用于文件名，不需要严格的日历格式。
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nailong-saves-{tag}-{}-{}",
            std::process::id(),
            timestamp()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_world(game_dir: &Path, name: &str) -> PathBuf {
        let dir = saves_dir(game_dir).join(name);
        fs::create_dir_all(dir.join("region")).unwrap();
        fs::write(dir.join("level.dat"), b"level").unwrap();
        fs::write(dir.join("region/r.0.0.mca"), b"region").unwrap();
        dir
    }

    #[test]
    fn lists_backs_up_and_restores_worlds() {
        let game = temp_dir("roundtrip");
        make_world(&game, "Test World");
        let list = worlds(&game).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Test World");
        assert!(list[0].size > 0);

        let backup = backup_world(&game, "Test World").unwrap();
        assert!(backup.is_file());
        let entries = backups(&game).unwrap();
        assert_eq!(entries.len(), 1);

        fs::remove_dir_all(saves_dir(&game).join("Test World")).unwrap();
        let restored = restore_backup(&game, &entries[0].name).unwrap();
        assert_eq!(restored, "Test World");
        assert!(saves_dir(&game).join("Test World/level.dat").is_file());
        assert!(
            saves_dir(&game)
                .join("Test World/region/r.0.0.mca")
                .is_file()
        );

        // 已存在时拒绝覆盖。
        assert!(restore_backup(&game, &entries[0].name).is_err());
        delete_backup(&game, &entries[0].name).unwrap();
        assert!(backups(&game).unwrap().is_empty());
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn imports_worlds_from_folders_and_zips_without_overwriting() {
        let source = temp_dir("import-src");
        let game = temp_dir("import-dst");
        // 一个世界文件夹。
        let folder = make_world(&source, "生存一号");
        assert!(is_world(&folder));
        assert!(!is_world(&source));
        assert_eq!(import_world(&game, &folder).unwrap(), "生存一号");
        // 再来一次：不覆盖，换个名字。
        assert_eq!(import_world(&game, &folder).unwrap(), "生存一号 (2)");
        assert!(
            saves_dir(&game)
                .join("生存一号 (2)/region/r.0.0.mca")
                .is_file()
        );

        // 备份出来的 zip：世界在包里的一个文件夹里。
        let zip_path = backup_world(&source, "生存一号").unwrap();
        assert!(is_world(&zip_path));
        assert_eq!(import_world(&game, &zip_path).unwrap(), "生存一号 (3)");

        // level.dat 直接在 zip 根上：用 zip 的名字。
        let flat = source.join("朋友的图.zip");
        let mut zip = ZipWriter::new(File::create(&flat).unwrap());
        let options = SimpleFileOptions::default();
        zip.start_file("level.dat", options).unwrap();
        zip.write_all(b"level").unwrap();
        zip.start_file("region/r.0.0.mca", options).unwrap();
        zip.write_all(b"region").unwrap();
        zip.finish().unwrap();
        assert_eq!(import_world(&game, &flat).unwrap(), "朋友的图");
        assert!(saves_dir(&game).join("朋友的图/region/r.0.0.mca").is_file());

        // 不是世界的 zip 认不出来，也导不进去。
        let other = source.join("pack.zip");
        let mut zip = ZipWriter::new(File::create(&other).unwrap());
        zip.start_file("pack.mcmeta", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
        assert!(!is_world(&other));
        assert!(import_world(&game, &other).is_err());
        assert_eq!(worlds(&game).unwrap().len(), 4);
        // 没留下半成品。
        assert!(
            fs::read_dir(saves_dir(&game))
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".part"))
        );
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn rejects_traversal_names() {
        let game = temp_dir("traversal");
        make_world(&game, "World");
        assert!(delete_world(&game, "../World").is_err());
        assert!(backup_world(&game, "../World").is_err());
        let _ = fs::remove_dir_all(&game);
    }
}
