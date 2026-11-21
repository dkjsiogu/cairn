//! 世界快照：像 git 一样按内容存，再细到区块。
//!
//! - 区域文件（`region/`、`entities/`、`poi/` 下的 `r.X.Z.mca`）按区块拆开，每个区块是一个对象，
//!   文件只记一张「区块表」（序号、保存时间、对象哈希）。没动过的区块两次快照共用一份，
//!   回档时也可以只把选中的几个区块换回去。
//! - 其他文件按内容分块（64 KiB–4 MiB）。
//! - 对象写进包文件（`repo/packs/*.pack` + `.idx`），不散成几十万个小文件。
//!   旧版本留下的散对象（`repo/objects/`）照样能读。

use crate::error::{Error, Result};
use crate::paths::Root;
use crate::region;
use crate::saves;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MIN_CHUNK: usize = 64 * 1024;
const MAX_CHUNK: usize = 4 * 1024 * 1024;
const MASK: u64 = (1 << 18) - 1;
const WINDOW: usize = 32;

/// 区域文件所在的三种目录。
const REGION_KINDS: [&str; 3] = ["region", "entities", "poi"];
const TABLE_MAGIC: &[u8; 4] = b"CRT1";
const TABLE_ROW: usize = 2 + 4 + 20;
const IDX_MAGIC: &[u8; 4] = b"CPK1";
const IDX_ROW: usize = 20 + 8 + 4;

/// 写快照、清理和回档区块不能同时进行：清理会删掉还没写进清单的新对象。
static REPO_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    REPO_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub enabled: bool,
    pub interval_minutes: u64,
    /// 每个世界保留多少份快照。
    pub keep: usize,
    pub last_run: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 30,
            keep: 20,
            last_run: 0,
        }
    }
}

pub fn load_config(root: &Root) -> Config {
    let path = root.dir().join("backups.json");
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_config(root: &Root, config: &Config) -> Result<()> {
    let path = root.dir().join("backups.json");
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|e| Error::Install(format!("备份配置序列化失败: {e}")))?;
    crate::paths::write_atomic(&path, &bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    /// 普通文件：按顺序拼起来就是原文件的那些块。区域文件为空。
    pub chunks: Vec<String>,
    /// 区域文件：区块表对象的哈希。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// 修改时间（毫秒）。大小和时间都没变的文件下次快照直接沿用，不再读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub world: String,
    pub id: String,
    pub created_at: u64,
    pub bytes: u64,
    pub files: Vec<FileEntry>,
    /// 不是玩家点的快照时写明来由，比如「回档区块前」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 空文件夹（有些 Mod 只认文件夹在不在）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<String>,
}

fn safe_name(name: &str) -> Result<&str> {
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name),
        _ => Err(Error::Install(format!("非法的存档名: {name}"))),
    }
}

fn repo_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("backups").join("repo")
}

fn objects_dir(game_dir: &Path) -> PathBuf {
    repo_dir(game_dir).join("objects")
}

fn packs_dir(game_dir: &Path) -> PathBuf {
    repo_dir(game_dir).join("packs")
}

fn snapshots_root(game_dir: &Path) -> PathBuf {
    repo_dir(game_dir).join("snapshots")
}

fn snapshots_dir(game_dir: &Path, world: &str) -> PathBuf {
    snapshots_root(game_dir).join(world)
}

// ───────────────────────── 对象库 ─────────────────────────

type Hash = [u8; 20];
/// 包里的一个对象：哈希、偏移、长度。
type PackRow = (Hash, u64, u32);

fn sha1(data: &[u8]) -> Hash {
    Sha1::digest(data).into()
}

fn hex(hash: &Hash) -> String {
    crate::http::hex(hash)
}

fn unhex(text: &str) -> Option<Hash> {
    if text.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

struct PackWriter {
    name: String,
    tmp: PathBuf,
    file: BufWriter<fs::File>,
    offset: u64,
    rows: Vec<PackRow>,
}

impl Drop for PackWriter {
    fn drop(&mut self) {
        // 没走到 finish（出错、取消）：半个包不留下。finish 之后 tmp 已经改名，删不到。
        let _ = fs::remove_file(&self.tmp);
    }
}

/// 一个游戏目录下的快照对象库。
struct Store {
    packs_dir: PathBuf,
    loose_dir: PathBuf,
    index: HashMap<Hash, (String, u64, u32)>,
    writer: Option<PackWriter>,
    open: HashMap<String, fs::File>,
}

impl Store {
    fn open(game_dir: &Path) -> Result<Self> {
        let packs_dir = packs_dir(game_dir);
        let mut index = HashMap::new();
        for (name, rows) in read_indexes(&packs_dir) {
            for (hash, offset, len) in rows {
                index.insert(hash, (name.clone(), offset, len));
            }
        }
        Ok(Self {
            packs_dir,
            loose_dir: objects_dir(game_dir),
            index,
            writer: None,
            open: HashMap::new(),
        })
    }

    fn loose_path(&self, hash: &Hash) -> PathBuf {
        let text = hex(hash);
        self.loose_dir.join(&text[..2]).join(&text[2..])
    }

    fn has(&self, hash: &Hash) -> bool {
        self.index.contains_key(hash) || self.loose_path(hash).is_file()
    }

    fn put(&mut self, data: &[u8]) -> Result<Hash> {
        let hash = sha1(data);
        if self.has(&hash) {
            return Ok(hash);
        }
        if self.writer.is_none() {
            fs::create_dir_all(&self.packs_dir)
                .map_err(|e| crate::error::io(&self.packs_dir, e))?;
            let name = format!("{}-{}", now_millis(), next_seq());
            let tmp = self.packs_dir.join(format!("{name}.pack.tmp"));
            let file = fs::File::create(&tmp).map_err(|e| crate::error::io(&tmp, e))?;
            self.writer = Some(PackWriter {
                name,
                tmp,
                file: BufWriter::new(file),
                offset: 0,
                rows: Vec::new(),
            });
        }
        let writer = self.writer.as_mut().expect("刚建好");
        writer
            .file
            .write_all(data)
            .map_err(|e| crate::error::io(&writer.tmp, e))?;
        writer.rows.push((hash, writer.offset, data.len() as u32));
        self.index.insert(
            hash,
            (writer.name.clone(), writer.offset, data.len() as u32),
        );
        writer.offset += data.len() as u64;
        Ok(hash)
    }

    /// 把这次新写的对象落成一个包。不调用就什么都不留。
    fn finish(&mut self) -> Result<()> {
        let Some(mut writer) = self.writer.take() else {
            return Ok(());
        };
        writer
            .file
            .flush()
            .map_err(|e| crate::error::io(&writer.tmp, e))?;
        writer
            .file
            .get_ref()
            .sync_all()
            .map_err(|e| crate::error::io(&writer.tmp, e))?;
        let pack = self.packs_dir.join(format!("{}.pack", writer.name));
        fs::rename(&writer.tmp, &pack).map_err(|e| crate::error::io(&pack, e))?;
        let idx = self.packs_dir.join(format!("{}.idx", writer.name));
        crate::paths::write_atomic(&idx, &encode_index(&writer.rows))
    }

    /// 取对象；库里没有时为 None。读到的内容会核对哈希。
    fn get_opt(&mut self, hash: &Hash) -> Result<Option<Vec<u8>>> {
        let data = if let Some((name, offset, len)) = self.index.get(hash).cloned() {
            let path = self.packs_dir.join(format!("{name}.pack"));
            if !self.open.contains_key(&name) {
                let file = fs::File::open(&path).map_err(|e| crate::error::io(&path, e))?;
                self.open.insert(name.clone(), file);
            }
            let file = self.open.get_mut(&name).expect("刚打开");
            let mut data = vec![0u8; len as usize];
            file.seek(SeekFrom::Start(offset))
                .and_then(|_| file.read_exact(&mut data))
                .map_err(|e| crate::error::io(&path, e))?;
            data
        } else {
            let path = self.loose_path(hash);
            match fs::read(&path) {
                Ok(data) => data,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(crate::error::io(&path, e)),
            }
        };
        if sha1(&data) != *hash {
            return Err(Error::Install(format!("快照数据损坏: {}", hex(hash))));
        }
        Ok(Some(data))
    }

    fn get(&mut self, hash: &Hash) -> Result<Vec<u8>> {
        self.get_opt(hash)?
            .ok_or_else(|| Error::Install(format!("快照缺少数据块: {}", hex(hash))))
    }

    fn get_hex(&mut self, text: &str) -> Result<Vec<u8>> {
        let hash = unhex(text).ok_or_else(|| Error::Install("快照里的块哈希无效".into()))?;
        self.get(&hash)
    }

    /// 这个文件记录引用的对象都还在（沿用上一份快照的记录前检查）。
    fn has_entry(&self, entry: &FileEntry) -> bool {
        match &entry.region {
            Some(table) => unhex(table).is_some_and(|h| self.has(&h)),
            None => entry
                .chunks
                .iter()
                .all(|c| unhex(c).is_some_and(|h| self.has(&h))),
        }
    }
}

fn encode_index(rows: &[PackRow]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + rows.len() * IDX_ROW);
    out.extend_from_slice(IDX_MAGIC);
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    for (hash, offset, len) in rows {
        out.extend_from_slice(hash);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
    }
    out
}

fn decode_index(data: &[u8]) -> Option<Vec<PackRow>> {
    if data.len() < 8 || &data[..4] != IDX_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if data.len() != 8 + count * IDX_ROW {
        return None;
    }
    Some(
        data[8..]
            .as_chunks::<IDX_ROW>()
            .0
            .iter()
            .map(|row| {
                let hash: Hash = row[..20].try_into().expect("20 字节");
                let offset = u64::from_le_bytes(row[20..28].try_into().expect("8 字节"));
                let len = u32::from_le_bytes(row[28..32].try_into().expect("4 字节"));
                (hash, offset, len)
            })
            .collect(),
    )
}

/// 每个包的名字和它里面的对象。坏掉的索引跳过。
fn read_indexes(dir: &Path) -> Vec<(String, Vec<PackRow>)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("idx") {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if let Some(rows) = fs::read(&path).ok().and_then(|data| decode_index(&data)) {
            out.push((name, rows));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 区块表的一行：序号、保存时间、数据的哈希。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    index: u16,
    timestamp: u32,
    hash: Hash,
}

fn encode_table(rows: &[Row]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + rows.len() * TABLE_ROW);
    out.extend_from_slice(TABLE_MAGIC);
    for row in rows {
        out.extend_from_slice(&row.index.to_le_bytes());
        out.extend_from_slice(&row.timestamp.to_le_bytes());
        out.extend_from_slice(&row.hash);
    }
    out
}

fn decode_table(data: &[u8]) -> Result<Vec<Row>> {
    if data.len() < 4 || &data[..4] != TABLE_MAGIC || !(data.len() - 4).is_multiple_of(TABLE_ROW) {
        return Err(Error::Install("快照里的区块表损坏".into()));
    }
    Ok(data[4..]
        .as_chunks::<TABLE_ROW>()
        .0
        .iter()
        .map(|row| Row {
            index: u16::from_le_bytes([row[0], row[1]]),
            timestamp: u32::from_le_bytes([row[2], row[3], row[4], row[5]]),
            hash: row[6..26].try_into().expect("20 字节"),
        })
        .collect())
}

/// 快照里一个区块的数据：在对象库里，或者（旧快照整文件存的）已经拆出来了。
enum Saved {
    Object(Hash),
    Inline(Vec<u8>),
}

struct SavedSlot {
    timestamp: u32,
    data: Saved,
}

impl SavedSlot {
    fn same_as(&self, payload: &[u8]) -> bool {
        match &self.data {
            Saved::Object(hash) => sha1(payload) == *hash,
            Saved::Inline(bytes) => bytes == payload,
        }
    }

    fn bytes(&self, store: &mut Store) -> Result<Vec<u8>> {
        match &self.data {
            Saved::Object(hash) => store.get(hash),
            Saved::Inline(bytes) => Ok(bytes.clone()),
        }
    }
}

/// 快照里一个区域文件的全部区块。旧快照把它当普通文件存，这里拼回来再拆。
fn saved_slots(store: &mut Store, entry: &FileEntry) -> Result<BTreeMap<u16, SavedSlot>> {
    let mut out = BTreeMap::new();
    if let Some(table) = &entry.region {
        for row in decode_table(&store.get_hex(table)?)? {
            out.insert(
                row.index,
                SavedSlot {
                    timestamp: row.timestamp,
                    data: Saved::Object(row.hash),
                },
            );
        }
        return Ok(out);
    }
    let bytes = assemble(store, entry)?;
    let slots = region::parse(&bytes)
        .ok_or_else(|| Error::Install(format!("快照里的 {} 不是完整的区域文件", entry.path)))?;
    for slot in slots {
        out.insert(
            slot.index,
            SavedSlot {
                timestamp: slot.timestamp,
                data: Saved::Inline(slot.payload),
            },
        );
    }
    Ok(out)
}

/// 普通文件：按顺序拼块。
fn assemble(store: &mut Store, entry: &FileEntry) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(entry.size as usize);
    for hash in &entry.chunks {
        bytes.extend_from_slice(&store.get_hex(hash)?);
    }
    if bytes.len() as u64 != entry.size {
        return Err(Error::Install(format!("快照文件大小不符: {}", entry.path)));
    }
    Ok(bytes)
}

/// 还原一个文件的完整内容。区域文件按区块重新排布，和原文件逐字节不同但内容一样。
fn file_bytes(store: &mut Store, entry: &FileEntry) -> Result<Vec<u8>> {
    if entry.region.is_none() {
        return assemble(store, entry);
    }
    let mut slots = Vec::new();
    for (index, saved) in saved_slots(store, entry)? {
        slots.push(region::Slot {
            index,
            timestamp: saved.timestamp,
            payload: saved.bytes(store)?,
        });
    }
    region::build(&slots)
}

/// 路径是不是某个维度下 `region/`、`entities/`、`poi/` 里的区域文件。
fn is_region_path(relative: &str) -> bool {
    let mut parts = relative.rsplit('/');
    let (Some(name), Some(kind)) = (parts.next(), parts.next()) else {
        return false;
    };
    REGION_KINDS.contains(&kind) && region::parse_name(name).is_some()
}

// ───────────────────────── 快照 ─────────────────────────

pub fn backup(game_dir: &Path, world: &str) -> Result<Snapshot> {
    let _guard = lock();
    backup_locked(game_dir, world, None)
}

fn backup_locked(game_dir: &Path, world: &str, note: Option<&str>) -> Result<Snapshot> {
    let world = safe_name(world)?;
    let source = saves::saves_dir(game_dir).join(world);
    if !source.join("level.dat").is_file() {
        return Err(Error::Install(format!("找不到存档: {world}")));
    }
    let previous: HashMap<String, FileEntry> = list(game_dir, world)?
        .into_iter()
        .next()
        .map(|snapshot| {
            snapshot
                .files
                .into_iter()
                .map(|file| (file.path.clone(), file))
                .collect()
        })
        .unwrap_or_default();
    let mut store = Store::open(game_dir)?;
    let mut files = Vec::new();
    let mut bytes = 0u64;
    walk(&source, Path::new(""), &mut |relative, path| {
        let relative = relative.to_string_lossy().replace('\\', "/");
        // 游戏开着时 Windows 会锁住它；恢复出来的世界也用不着。
        if relative == "session.lock" {
            return Ok(());
        }
        let meta = fs::metadata(path).map_err(|e| crate::error::io(path, e))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        if let Some(old) = previous.get(&relative)
            && modified.is_some()
            && old.modified == modified
            && old.size == meta.len()
            && store.has_entry(old)
        {
            bytes += old.size;
            files.push(old.clone());
            return Ok(());
        }
        let mut data = Vec::new();
        fs::File::open(path)
            .and_then(|mut file| file.read_to_end(&mut data))
            .map_err(|e| crate::error::io(path, e))?;
        bytes += data.len() as u64;
        let slots = if is_region_path(&relative) {
            region::parse(&data)
        } else {
            None
        };
        let entry = if let Some(slots) = slots {
            let mut rows = Vec::with_capacity(slots.len());
            for slot in &slots {
                rows.push(Row {
                    index: slot.index,
                    timestamp: slot.timestamp,
                    hash: store.put(&slot.payload)?,
                });
            }
            let table = store.put(&encode_table(&rows))?;
            FileEntry {
                path: relative,
                size: data.len() as u64,
                chunks: Vec::new(),
                region: Some(hex(&table)),
                modified,
            }
        } else {
            let mut chunks = Vec::new();
            for chunk in split_chunks(&data) {
                chunks.push(hex(&store.put(chunk)?));
            }
            FileEntry {
                path: relative,
                size: data.len() as u64,
                chunks,
                region: None,
                modified,
            }
        };
        files.push(entry);
        Ok(())
    })?;
    store.finish()?;
    let snapshot = Snapshot {
        world: world.to_string(),
        id: format!("{}-{}", now_millis(), next_seq()),
        created_at: now(),
        bytes,
        files,
        note: note.map(str::to_string),
        dirs: empty_dirs(&source, Path::new("")),
    };
    let dir = snapshots_dir(game_dir, world);
    fs::create_dir_all(&dir).map_err(|e| crate::error::io(&dir, e))?;
    let path = dir.join(format!("{}.json", snapshot.id));
    let data = serde_json::to_vec(&snapshot)
        .map_err(|e| Error::Install(format!("快照序列化失败: {e}")))?;
    crate::paths::write_atomic(&path, &data)?;
    Ok(snapshot)
}

pub fn list(game_dir: &Path, world: &str) -> Result<Vec<Snapshot>> {
    let world = safe_name(world)?;
    let dir = snapshots_dir(game_dir, world);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(data) = fs::read(&path) else {
            continue;
        };
        if let Ok(snapshot) = serde_json::from_slice::<Snapshot>(&data) {
            out.push(snapshot);
        }
    }
    // created_at 只到秒；同一秒内的多次备份用 id（毫秒-序号）兜底，
    // 否则 read_dir 的顺序会让“最新快照”不稳定，prune 可能删错。
    out.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    Ok(out)
}

fn load(game_dir: &Path, world: &str, id: &str) -> Result<Snapshot> {
    let world = safe_name(world)?;
    let id = safe_name(id)?;
    let path = snapshots_dir(game_dir, world).join(format!("{id}.json"));
    let data = fs::read(&path).map_err(|e| crate::error::io(&path, e))?;
    serde_json::from_slice(&data).map_err(|e| Error::Install(format!("快照损坏: {e}")))
}

/// 只看文件名统计快照：份数和最新一份的时间（秒）。
///
/// 快照 id 是“毫秒-序号”，不用解析整份清单，启动台可以随时调用。
pub fn summary(game_dir: &Path, world: &str) -> Result<(usize, Option<u64>)> {
    let world = safe_name(world)?;
    let Ok(entries) = fs::read_dir(snapshots_dir(game_dir, world)) else {
        return Ok((0, None));
    };
    let mut count = 0;
    let mut latest: Option<u64> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        let millis = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.split('-').next())
            .and_then(|millis| millis.parse::<u64>().ok());
        if let Some(secs) = millis.map(|millis| millis / 1000) {
            latest = Some(latest.map_or(secs, |current| current.max(secs)));
        }
    }
    Ok((count, latest))
}

/// 整个世界恢复到 `saves/<world>`；目录已存在时改名为 `<world>-restored-<时间>`。
pub fn restore(game_dir: &Path, world: &str, id: &str) -> Result<PathBuf> {
    let snapshot = load(game_dir, world, id)?;
    let saves = saves::saves_dir(game_dir);
    fs::create_dir_all(&saves).map_err(|e| crate::error::io(&saves, e))?;
    let mut target = saves.join(&snapshot.world);
    if target.exists() {
        target = saves.join(format!("{}-restored-{}", snapshot.world, now()));
    }
    let mut store = Store::open(game_dir)?;
    for file in &snapshot.files {
        let relative = safe_relative(&file.path)?;
        let dest = target.join(&relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| crate::error::io(parent, e))?;
        }
        let bytes = file_bytes(&mut store, file)?;
        fs::write(&dest, &bytes).map_err(|e| crate::error::io(&dest, e))?;
    }
    for dir in &snapshot.dirs {
        let dest = target.join(safe_relative(dir)?);
        fs::create_dir_all(&dest).map_err(|e| crate::error::io(&dest, e))?;
    }
    Ok(target)
}

/// 对象库的对象数和占用字节数（用于界面展示“仓库”规模）。
pub fn repo_stats(game_dir: &Path) -> (usize, u64) {
    let mut count = 0usize;
    let mut bytes = 0u64;
    if let Ok(prefixes) = fs::read_dir(objects_dir(game_dir)) {
        for prefix in prefixes.flatten() {
            if let Ok(entries) = fs::read_dir(prefix.path()) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata()
                        && meta.is_file()
                    {
                        count += 1;
                        bytes += meta.len();
                    }
                }
            }
        }
    }
    let packs = packs_dir(game_dir);
    for (name, rows) in read_indexes(&packs) {
        count += rows.len();
        bytes += fs::metadata(packs.join(format!("{name}.pack"))).map_or(0, |m| m.len());
    }
    (count, bytes)
}

pub fn delete(game_dir: &Path, world: &str, id: &str) -> Result<()> {
    let world = safe_name(world)?;
    let id = safe_name(id)?;
    let path = snapshots_dir(game_dir, world).join(format!("{id}.json"));
    fs::remove_file(&path).map_err(|e| crate::error::io(&path, e))
}

/// 每个世界只保留最新 N 份快照，并清掉没有任何快照引用的对象。
///
/// 按快照仓库里的目录走，不看现在 `saves/` 里还有哪些世界：
/// 删掉的世界留下的快照照样保留、照样算引用，之后还能整份恢复。
pub fn prune(game_dir: &Path, keep: usize) -> Result<(usize, usize)> {
    let _guard = lock();
    let mut removed = 0usize;
    let mut kept = Vec::new();
    if let Ok(entries) = fs::read_dir(snapshots_root(game_dir)) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(items) = list(game_dir, &name) else {
                continue;
            };
            for (index, snapshot) in items.into_iter().enumerate() {
                if index >= keep.max(1) {
                    let path = snapshots_dir(game_dir, &name).join(format!("{}.json", snapshot.id));
                    if fs::remove_file(path).is_ok() {
                        removed += 1;
                    }
                } else {
                    kept.push(snapshot);
                }
            }
        }
    }
    // 读不了的清单（不是 JSON、正在写）不会出现在 kept 里；它引用的对象一旦被删就再也恢复不了。
    // 发现这种清单就只删旧快照、不回收对象。
    if has_unreadable_manifest(game_dir) {
        return Ok((removed, 0));
    }

    let mut store = Store::open(game_dir)?;
    let mut referenced: HashSet<Hash> = HashSet::new();
    for snapshot in &kept {
        for file in &snapshot.files {
            for chunk in &file.chunks {
                if let Some(hash) = unhex(chunk) {
                    referenced.insert(hash);
                }
            }
            if let Some(table) = file.region.as_deref().and_then(unhex) {
                referenced.insert(table);
                // 区块表丢了的快照本来就坏了；表在但读不出来时不敢回收，整个停下。
                if let Some(data) = store.get_opt(&table)? {
                    for row in decode_table(&data)? {
                        referenced.insert(row.hash);
                    }
                }
            }
        }
    }
    drop(store);

    let mut deleted = 0usize;
    let objects = objects_dir(game_dir);
    if let Ok(prefixes) = fs::read_dir(&objects) {
        for prefix in prefixes.flatten() {
            let dir = prefix.path();
            if !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let full = format!(
                        "{}{}",
                        prefix.file_name().to_string_lossy(),
                        entry.file_name().to_string_lossy()
                    );
                    if unhex(&full).is_none_or(|hash| !referenced.contains(&hash))
                        && fs::remove_file(entry.path()).is_ok()
                    {
                        deleted += 1;
                    }
                }
            }
        }
    }
    deleted += repack(game_dir, &referenced)?;
    Ok((removed, deleted))
}

fn has_unreadable_manifest(game_dir: &Path) -> bool {
    let Ok(worlds) = fs::read_dir(snapshots_root(game_dir)) else {
        return false;
    };
    worlds.flatten().any(|world| {
        fs::read_dir(world.path()).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                let path = entry.path();
                path.extension().and_then(|e| e.to_str()) == Some("json")
                    && fs::read(&path)
                        .ok()
                        .and_then(|data| serde_json::from_slice::<Snapshot>(&data).ok())
                        .is_none()
            })
        })
    })
}

/// 包里全是没用的对象就删掉；没用的占到四分之一以上就把有用的挪进新包。返回扔掉的对象数。
fn repack(game_dir: &Path, referenced: &HashSet<Hash>) -> Result<usize> {
    let dir = packs_dir(game_dir);
    let indexes = read_indexes(&dir);
    let named: HashSet<&str> = indexes.iter().map(|(name, _)| name.as_str()).collect();
    // 没有索引的包、没写完的临时文件：都是中途出错留下的。
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            let orphan = file
                .strip_suffix(".pack")
                .is_some_and(|name| !named.contains(name));
            if orphan || file.ends_with(".tmp") || file.contains(".tmp-") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let mut dropped = 0usize;
    let mut store = Store::open(game_dir)?;
    for (name, rows) in &indexes {
        let live: Vec<&PackRow> = rows.iter().filter(|r| referenced.contains(&r.0)).collect();
        let dead = rows.len() - live.len();
        if dead == 0 {
            continue;
        }
        let total: u64 = rows.iter().map(|r| r.2 as u64).sum();
        let waste: u64 = rows
            .iter()
            .filter(|r| !referenced.contains(&r.0))
            .map(|r| r.2 as u64)
            .sum();
        if !live.is_empty() && waste * 4 < total {
            continue;
        }
        if !live.is_empty() {
            // 先把有用的写进新包；新包落盘之后才删旧包。
            let pack = dir.join(format!("{name}.pack"));
            let mut file = fs::File::open(&pack).map_err(|e| crate::error::io(&pack, e))?;
            for (hash, offset, len) in live {
                let mut data = vec![0u8; *len as usize];
                file.seek(SeekFrom::Start(*offset))
                    .and_then(|_| file.read_exact(&mut data))
                    .map_err(|e| crate::error::io(&pack, e))?;
                if sha1(&data) != *hash {
                    return Err(Error::Install(format!("快照数据损坏: {}", hex(hash))));
                }
                // 旧包还在索引里，put 会以为已经有了；先从索引里拿掉。
                store.index.remove(hash);
                store.put(&data)?;
            }
            store.finish()?;
        }
        let _ = fs::remove_file(dir.join(format!("{name}.idx")));
        let _ = fs::remove_file(dir.join(format!("{name}.pack")));
        store.open.remove(name);
        dropped += dead;
    }
    Ok(dropped)
}

// ───────────────────────── 按区块回档 ─────────────────────────

/// 快照里的一个维度：`dir` 是相对世界目录的路径（主世界为空）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dimension {
    pub dir: String,
    pub label: String,
}

fn dimension_label(dir: &str) -> String {
    match dir {
        "" | "dimensions/minecraft/overworld" => "主世界".into(),
        "DIM-1" | "dimensions/minecraft/the_nether" => "下界".into(),
        "DIM1" | "dimensions/minecraft/the_end" => "末地".into(),
        other => match other.strip_prefix("dimensions/") {
            Some(rest) => rest.replacen('/', ":", 1),
            None => other.to_string(),
        },
    }
}

/// 快照里有地形的维度，主世界、下界、末地在前。
pub fn dimensions(snapshot: &Snapshot) -> Vec<Dimension> {
    let mut dirs = BTreeSet::new();
    for file in &snapshot.files {
        let Some(head) = file
            .path
            .strip_suffix(file.path.rsplit('/').next().unwrap_or(""))
        else {
            continue;
        };
        if !is_region_path(&file.path) {
            continue;
        }
        let Some(dir) = head.strip_suffix("region/") else {
            continue;
        };
        dirs.insert(dir.trim_end_matches('/').to_string());
    }
    let rank = |dir: &str| match dimension_label(dir).as_str() {
        "主世界" => 0,
        "下界" => 1,
        "末地" => 2,
        _ => 3,
    };
    let mut out: Vec<Dimension> = dirs
        .into_iter()
        .map(|dir| Dimension {
            label: dimension_label(&dir),
            dir,
        })
        .collect();
    out.sort_by(|a, b| (rank(&a.dir), &a.dir).cmp(&(rank(&b.dir), &b.dir)));
    out
}

/// 区块和快照比起来怎么样了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkState {
    /// 快照之后没再保存过。
    Same,
    /// 快照之后保存过。
    Changed,
    /// 快照之后才生成。回档会把它删掉，下次进游戏重新生成。
    Added,
    /// 快照里有、现在没有。
    Removed,
}

impl ChunkState {
    pub fn differs(self) -> bool {
        self != ChunkState::Same
    }
}

/// 一个维度里每个区块的状态，按保存时间比，只读文件头，很快。
#[derive(Debug, Clone, Default)]
pub struct ChunkMap {
    pub chunks: HashMap<(i32, i32), ChunkState>,
    /// 有变化的区块数。
    pub changed: usize,
    /// 变化最集中的地方（区块坐标），地图一打开先看这里。
    pub focus: (i32, i32),
}

fn join_dir(dim: &str, kind: &str) -> String {
    if dim.is_empty() {
        kind.to_string()
    } else {
        format!("{dim}/{kind}")
    }
}

fn check_dim(dim: &str) -> Result<()> {
    if !dim.is_empty() {
        safe_relative(dim)?;
    }
    Ok(())
}

pub fn chunk_map(game_dir: &Path, world: &str, id: &str, dim: &str) -> Result<ChunkMap> {
    check_dim(dim)?;
    let snapshot = load(game_dir, world, id)?;
    let world_dir = saves::saves_dir(game_dir).join(&snapshot.world);
    let prefix = format!("{}/", join_dir(dim, "region"));
    let mut saved: HashMap<(i32, i32), u32> = HashMap::new();
    let mut store = Store::open(game_dir)?;
    for file in &snapshot.files {
        let Some(name) = file.path.strip_prefix(&prefix) else {
            continue;
        };
        let Some((rx, rz)) = region::parse_name(name) else {
            continue;
        };
        for (index, slot) in saved_slots(&mut store, file)? {
            saved.insert(region::chunk_at(rx, rz, index), slot.timestamp);
        }
    }
    let mut now: HashMap<(i32, i32), u32> = HashMap::new();
    if let Ok(entries) = fs::read_dir(world_dir.join(join_dir(dim, "region"))) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some((rx, rz)) = region::parse_name(&name) else {
                continue;
            };
            for (index, timestamp) in region::header(&entry.path()) {
                now.insert(region::chunk_at(rx, rz, index), timestamp);
            }
        }
    }
    let mut map = ChunkMap::default();
    for (&pos, &then) in &saved {
        let state = match now.get(&pos) {
            Some(&current) if current == then => ChunkState::Same,
            Some(_) => ChunkState::Changed,
            None => ChunkState::Removed,
        };
        map.chunks.insert(pos, state);
    }
    for &pos in now.keys() {
        map.chunks.entry(pos).or_insert(ChunkState::Added);
    }
    // 落在哪个 8×8 的格子里的改动最多，就先看那里。
    let mut buckets: HashMap<(i32, i32), usize> = HashMap::new();
    for (&(x, z), state) in &map.chunks {
        if state.differs() {
            map.changed += 1;
            *buckets.entry((x >> 3, z >> 3)).or_default() += 1;
        }
    }
    map.focus = buckets
        .into_iter()
        .max_by_key(|&(pos, count)| (count, std::cmp::Reverse(pos)))
        .map(|((x, z), _)| (x * 8 + 4, z * 8 + 4))
        .or_else(|| {
            let n = map.chunks.len().max(1) as i64;
            let (sx, sz) = map
                .chunks
                .keys()
                .fold((0i64, 0i64), |(a, b), &(x, z)| (a + x as i64, b + z as i64));
            (!map.chunks.is_empty()).then(|| ((sx / n) as i32, (sz / n) as i32))
        })
        .unwrap_or((0, 0));
    Ok(map)
}

/// 要回档的一片区块：维度目录和两个角（区块坐标，含两端）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    pub dim: String,
    pub min: (i32, i32),
    pub max: (i32, i32),
}

impl Area {
    pub fn new(dim: &str, a: (i32, i32), b: (i32, i32)) -> Self {
        Self {
            dim: dim.to_string(),
            min: (a.0.min(b.0), a.1.min(b.1)),
            max: (a.0.max(b.0), a.1.max(b.1)),
        }
    }

    pub fn contains(&self, (x, z): (i32, i32)) -> bool {
        x >= self.min.0 && x <= self.max.0 && z >= self.min.1 && z <= self.max.1
    }

    pub fn count(&self) -> u64 {
        (self.max.0 - self.min.0 + 1) as u64 * (self.max.1 - self.min.1 + 1) as u64
    }
}

#[derive(Debug, Clone)]
pub struct ChunkRestore {
    /// 实际换回去的区块数（地形、实体、兴趣点任何一样不同都算）。
    pub chunks: usize,
    /// 动手之前自动做的那份快照，想撤销就恢复它。
    pub before: Option<String>,
}

/// 把世界里 `area` 这片区块换回快照里的样子：地形、实体和兴趣点一起换。
/// 快照里没有的区块会被删掉，下次进游戏重新生成。动手之前先自动快照一份当前的世界。
pub fn restore_chunks(game_dir: &Path, world: &str, id: &str, area: &Area) -> Result<ChunkRestore> {
    check_dim(&area.dim)?;
    let _guard = lock();
    let snapshot = load(game_dir, world, id)?;
    let world_dir = saves::saves_dir(game_dir).join(&snapshot.world);
    if !world_dir.join("level.dat").is_file() {
        return Err(Error::Install(
            "这个世界已经不在了，先整份恢复再回档区块".into(),
        ));
    }
    if world_in_use(&world_dir) {
        return Err(Error::Install(
            "游戏正在用这个世界，退出到标题画面再回档".into(),
        ));
    }
    let by_path: HashMap<&str, &FileEntry> = snapshot
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let mut store = Store::open(game_dir)?;
    let mut writes: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut changed: BTreeSet<(i32, i32)> = BTreeSet::new();
    let (rx0, rz0) = region::region_of(area.min.0, area.min.1);
    let (rx1, rz1) = region::region_of(area.max.0, area.max.1);
    for kind in REGION_KINDS {
        let dir_rel = join_dir(&area.dim, kind);
        for rx in rx0..=rx1 {
            for rz in rz0..=rz1 {
                let name = region::file_name(rx, rz);
                let rel = format!("{dir_rel}/{name}");
                let saved = match by_path.get(rel.as_str()) {
                    Some(entry) => saved_slots(&mut store, entry)?,
                    None => BTreeMap::new(),
                };
                let path = world_dir.join(&dir_rel).join(&name);
                let mut current: BTreeMap<u16, region::Slot> = BTreeMap::new();
                if path.is_file() {
                    let data = fs::read(&path).map_err(|e| crate::error::io(&path, e))?;
                    let slots = region::parse(&data)
                        .ok_or_else(|| Error::Install(format!("{rel} 读不懂，没有改动任何文件")))?;
                    current = slots.into_iter().map(|s| (s.index, s)).collect();
                }
                let mut touched = false;
                for cz in (rz * 32).max(area.min.1)..=(rz * 32 + 31).min(area.max.1) {
                    for cx in (rx * 32).max(area.min.0)..=(rx * 32 + 31).min(area.max.0) {
                        let index = region::index_of(cx, cz);
                        let want = saved.get(&index);
                        let have = current.get(&index);
                        let same = match (want, have) {
                            (None, None) => true,
                            (Some(w), Some(h)) => w.same_as(&h.payload),
                            _ => false,
                        };
                        if same {
                            continue;
                        }
                        touched = true;
                        changed.insert((cx, cz));
                        let external = world_dir.join(&dir_rel).join(region::external_name(cx, cz));
                        if have.is_some_and(|h| region::is_external(&h.payload)) {
                            writes.push((external.clone(), None));
                        }
                        match want {
                            Some(w) => {
                                let payload = w.bytes(&mut store)?;
                                if region::is_external(&payload) {
                                    let mcc =
                                        format!("{dir_rel}/{}", region::external_name(cx, cz));
                                    let entry = by_path.get(mcc.as_str()).ok_or_else(|| {
                                        Error::Install(format!("快照里缺少 {mcc}"))
                                    })?;
                                    writes.push((external, Some(file_bytes(&mut store, entry)?)));
                                }
                                current.insert(
                                    index,
                                    region::Slot {
                                        index,
                                        timestamp: w.timestamp,
                                        payload,
                                    },
                                );
                            }
                            None => {
                                current.remove(&index);
                            }
                        }
                    }
                }
                if touched {
                    let slots: Vec<region::Slot> = current.into_values().collect();
                    writes.push((path, Some(region::build(&slots)?)));
                }
            }
        }
    }
    if changed.is_empty() {
        return Ok(ChunkRestore {
            chunks: 0,
            before: None,
        });
    }
    let before = backup_locked(game_dir, &snapshot.world, Some("回档区块前"))?;
    for (path, data) in writes {
        match data {
            Some(bytes) => crate::paths::write_atomic(&path, &bytes)?,
            None => match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(crate::error::io(&path, e)),
            },
        }
    }
    Ok(ChunkRestore {
        chunks: changed.len(),
        before: Some(before.id),
    })
}

/// 游戏打开世界时会锁住 `session.lock`。Windows 上锁是强制的，能直接看出来；
/// 其他系统上 Java 的锁和这里的锁互不相认，靠界面那边「这个版本在运行」的判断。
fn world_in_use(world_dir: &Path) -> bool {
    let path = world_dir.join("session.lock");
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(fs::TryLockError::WouldBlock) => true,
        Err(fs::TryLockError::Error(_)) => false,
    }
}

// ───────────────────────── 工具 ─────────────────────────

fn safe_relative(path: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err(Error::Install(format!("快照里有越界路径: {path}"))),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(Error::Install("快照里有空路径".into()));
    }
    Ok(out)
}

/// 里面什么都没有的文件夹，相对世界目录。
fn empty_dirs(root: &Path, relative: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    let mut any = false;
    for entry in entries.flatten() {
        any = true;
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            out.extend(empty_dirs(&entry.path(), &relative.join(entry.file_name())));
        }
    }
    if !any && !relative.as_os_str().is_empty() {
        out.push(relative.to_string_lossy().replace('\\', "/"));
    }
    out.sort();
    out
}

fn walk(
    root: &Path,
    relative: &Path,
    handle: &mut impl FnMut(&Path, &Path) -> Result<()>,
) -> Result<()> {
    let entries = fs::read_dir(root).map_err(|e| crate::error::io(root, e))?;
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|entry| entry.file_name());
    for entry in items {
        let path = entry.path();
        let inner = relative.join(entry.file_name());
        let meta = entry.metadata().map_err(|e| crate::error::io(&path, e))?;
        if meta.is_dir() {
            walk(&path, &inner, handle)?;
        } else if meta.is_file() && meta.len() <= 512 * 1024 * 1024 {
            handle(&inner, &path)?;
        }
    }
    Ok(())
}

fn split_chunks(data: &[u8]) -> Vec<&[u8]> {
    let gear = gear_table();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut hash: u64 = 0;
    let mut index = 0usize;
    while index < data.len() {
        hash = hash.rotate_left(1).wrapping_add(gear[data[index] as usize]);
        if index >= WINDOW {
            hash ^= gear[data[index - WINDOW] as usize].rotate_left(WINDOW as u32);
        }
        let len = index - start + 1;
        if len >= MAX_CHUNK || (len >= MIN_CHUNK && (hash & MASK) == 0) {
            out.push(&data[start..=index]);
            start = index + 1;
            hash = 0;
        }
        index += 1;
    }
    if start < data.len() {
        out.push(&data[start..]);
    }
    out
}

fn gear_table() -> &'static [u64; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut table = [0u64; 256];
        for slot in &mut table {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *slot = state;
        }
        table
    })
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn next_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nailong-snap-{tag}-{}-{}",
            std::process::id(),
            now_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_world(game: &Path, name: &str, payload: &[u8]) {
        let dir = saves::saves_dir(game).join(name).join("region");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            saves::saves_dir(game).join(name).join("level.dat"),
            b"level",
        )
        .unwrap();
        fs::write(dir.join("r.0.0.mca"), payload).unwrap();
    }

    /// 区块内容：压缩方式 2（zlib）后跟可辨认的字节。
    fn chunk(tag: u8, len: usize) -> Vec<u8> {
        let mut out = vec![2u8];
        out.extend(std::iter::repeat_n(tag, len));
        out
    }

    fn write_region(game: &Path, world: &str, kind: &str, name: &str, slots: &[region::Slot]) {
        let dir = saves::saves_dir(game).join(world).join(kind);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            saves::saves_dir(game).join(world).join("level.dat"),
            b"level",
        )
        .unwrap();
        fs::write(dir.join(name), region::build(slots).unwrap()).unwrap();
        // 同一毫秒里写两次，修改时间可能不变；让快照不能偷懒沿用上一份。
        bump_mtime(&dir.join(name));
    }

    fn bump_mtime(path: &Path) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        let t = SystemTime::now() + std::time::Duration::from_secs(next_seq() + 1);
        file.set_modified(t).unwrap();
    }

    fn read_region(game: &Path, world: &str, kind: &str, name: &str) -> BTreeMap<u16, Vec<u8>> {
        let data = fs::read(saves::saves_dir(game).join(world).join(kind).join(name)).unwrap();
        region::parse(&data)
            .unwrap()
            .into_iter()
            .map(|s| (s.index, s.payload))
            .collect()
    }

    fn slot(index: u16, timestamp: u32, payload: Vec<u8>) -> region::Slot {
        region::Slot {
            index,
            timestamp,
            payload,
        }
    }

    #[test]
    fn summary_counts_snapshots_from_file_names() {
        let game_dir = temp_dir("summary");
        let dir = snapshots_dir(&game_dir, "World");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("1700000000000-1.json"), b"{}").unwrap();
        fs::write(dir.join("1700000600999-2.json"), b"{}").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        assert_eq!(
            summary(&game_dir, "World").unwrap(),
            (2, Some(1_700_000_600))
        );
        assert_eq!(summary(&game_dir, "Missing").unwrap(), (0, None));
        assert!(summary(&game_dir, "../escape").is_err());
        let _ = fs::remove_dir_all(&game_dir);
    }

    #[test]
    fn chunks_are_content_defined_and_shared() {
        let mut first = vec![7u8; 900_000];
        let chunks_a = split_chunks(&first);
        assert!(chunks_a.len() > 1);
        let set_a: BTreeSet<Vec<u8>> = chunks_a.iter().map(|c| c.to_vec()).collect();
        // 中间改一小段，大部分块应该保持不变（内容定义分块）。
        first[400_000..400_100].fill(9);
        let chunks_b = split_chunks(&first);
        let shared = chunks_b
            .iter()
            .filter(|c| set_a.iter().any(|x| x.as_slice() == **c))
            .count();
        assert!(
            shared + 2 >= chunks_b.len(),
            "共享块太少: {shared}/{}",
            chunks_b.len()
        );
    }

    #[test]
    fn snapshots_dedupe_and_restore() {
        let game = temp_dir("roundtrip");
        // 不是合法的区域文件：按普通文件分块存。
        let mut payload = vec![1u8; 700_000];
        write_world(&game, "World", &payload);
        fs::create_dir_all(saves::saves_dir(&game).join("World/datapacks")).unwrap();
        let first = backup(&game, "World").unwrap();
        assert_eq!(first.dirs, ["datapacks"]);
        let (objects_after_first, _) = repo_stats(&game);
        payload[300_000..300_050].fill(2);
        write_world(&game, "World", &payload);
        bump_mtime(&saves::saves_dir(&game).join("World/region/r.0.0.mca"));
        let second = backup(&game, "World").unwrap();
        let (objects_after_second, _) = repo_stats(&game);
        assert!(second.id != first.id);
        assert!(
            objects_after_second < objects_after_first * 2,
            "没有去重: {objects_after_first} -> {objects_after_second}"
        );
        assert_eq!(list(&game, "World").unwrap().len(), 2);

        fs::remove_dir_all(saves::saves_dir(&game).join("World")).unwrap();
        let target = restore(&game, "World", &second.id).unwrap();
        assert!(target.join("level.dat").is_file());
        assert!(target.join("datapacks").is_dir());
        let restored = fs::read(target.join("region/r.0.0.mca")).unwrap();
        assert_eq!(restored.len(), payload.len());
        assert_eq!(restored[300_000], 2);
        assert_eq!(restored[500_000], 1);
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn region_files_are_stored_per_chunk() {
        let game = temp_dir("per-chunk");
        let mut slots: Vec<region::Slot> = (0..40)
            .map(|i| slot(i, 100, chunk(i as u8, 3000)))
            .collect();
        write_region(&game, "World", "region", "r.0.0.mca", &slots);
        backup(&game, "World").unwrap();
        let (objects_first, _) = repo_stats(&game);
        // 改一个区块：只多出这个区块和一张新表。
        slots[7] = slot(7, 200, chunk(200, 3000));
        write_region(&game, "World", "region", "r.0.0.mca", &slots);
        let second = backup(&game, "World").unwrap();
        let (objects_second, _) = repo_stats(&game);
        assert_eq!(objects_second, objects_first + 2);
        assert!(second.files.iter().any(|f| f.region.is_some()));

        fs::remove_dir_all(saves::saves_dir(&game).join("World")).unwrap();
        restore(&game, "World", &second.id).unwrap();
        let back = read_region(&game, "World", "region", "r.0.0.mca");
        assert_eq!(back.len(), 40);
        assert_eq!(back[&7], chunk(200, 3000));
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn unchanged_files_are_reused_without_reading() {
        let game = temp_dir("reuse");
        let slots: Vec<region::Slot> = (0..4).map(|i| slot(i, 1, chunk(i as u8, 100))).collect();
        write_region(&game, "World", "region", "r.0.0.mca", &slots);
        let first = backup(&game, "World").unwrap();
        let second = backup(&game, "World").unwrap();
        let region_of = |s: &Snapshot| {
            s.files
                .iter()
                .find(|f| f.path == "region/r.0.0.mca")
                .unwrap()
                .region
                .clone()
        };
        assert_eq!(region_of(&first), region_of(&second));
        assert!(first.files.iter().all(|f| f.modified.is_some()));
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn restore_chunks_replaces_only_the_area() {
        let game = temp_dir("area");
        // 区块 (0..4, 0) 在主世界，实体文件里也有同样的区块。
        let base: Vec<region::Slot> = (0..4).map(|i| slot(i, 10, chunk(i as u8, 500))).collect();
        write_region(&game, "World", "region", "r.0.0.mca", &base);
        write_region(&game, "World", "entities", "r.0.0.mca", &base);
        let snap = backup(&game, "World").unwrap();

        // 玩家炸掉了 1、2 号区块，又新走到 9 号区块。
        let mut later = base.clone();
        later[1] = slot(1, 20, chunk(101, 500));
        later[2] = slot(2, 20, chunk(102, 500));
        later.push(slot(9, 20, chunk(109, 500)));
        write_region(&game, "World", "region", "r.0.0.mca", &later);
        write_region(&game, "World", "entities", "r.0.0.mca", &later);

        let map = chunk_map(&game, "World", &snap.id, "").unwrap();
        assert_eq!(map.chunks[&(0, 0)], ChunkState::Same);
        assert_eq!(map.chunks[&(1, 0)], ChunkState::Changed);
        assert_eq!(map.chunks[&(9, 0)], ChunkState::Added);
        assert_eq!(map.changed, 3);

        // 只回档 (1,0)：(2,0) 和 (9,0) 保持现在的样子。
        let done =
            restore_chunks(&game, "World", &snap.id, &Area::new("", (1, 0), (1, 0))).unwrap();
        assert_eq!(done.chunks, 1);
        assert!(done.before.is_some());
        for kind in ["region", "entities"] {
            let now = read_region(&game, "World", kind, "r.0.0.mca");
            assert_eq!(now[&1], chunk(1, 500));
            assert_eq!(now[&2], chunk(102, 500));
            assert_eq!(now[&9], chunk(109, 500));
        }

        // 再回档 (2..9, 0)：2 号换回去，9 号快照里没有，删掉。
        let done =
            restore_chunks(&game, "World", &snap.id, &Area::new("", (9, 0), (2, 0))).unwrap();
        assert_eq!(done.chunks, 2);
        let now = read_region(&game, "World", "region", "r.0.0.mca");
        assert_eq!(now.len(), 4);
        assert_eq!(now[&2], chunk(2, 500));

        // 什么都没变时不动文件，也不多做快照。
        let before = list(&game, "World").unwrap().len();
        let done =
            restore_chunks(&game, "World", &snap.id, &Area::new("", (0, 0), (3, 0))).unwrap();
        assert_eq!(done.chunks, 0);
        assert_eq!(list(&game, "World").unwrap().len(), before);

        // 回档前的那份快照能把炸掉的样子找回来。
        let undo = list(&game, "World")
            .unwrap()
            .into_iter()
            .rfind(|s| s.note.as_deref() == Some("回档区块前"))
            .unwrap();
        restore_chunks(&game, "World", &undo.id, &Area::new("", (0, 0), (31, 31))).unwrap();
        let now = read_region(&game, "World", "region", "r.0.0.mca");
        assert_eq!(now[&1], chunk(101, 500));
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn restore_chunks_from_old_whole_file_snapshot() {
        let game = temp_dir("legacy");
        let base: Vec<region::Slot> = (0..3).map(|i| slot(i, 10, chunk(i as u8, 500))).collect();
        write_region(&game, "World", "region", "r.0.0.mca", &base);
        // 旧版本的快照：区域文件当普通文件分块存，对象散放在 objects/ 下。
        let data = region::build(&base).unwrap();
        let mut chunks = Vec::new();
        for piece in split_chunks(&data) {
            let hash = sha1(piece);
            let text = hex(&hash);
            let path = objects_dir(&game).join(&text[..2]).join(&text[2..]);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, piece).unwrap();
            chunks.push(text);
        }
        let old = Snapshot {
            world: "World".into(),
            id: "1-0".into(),
            created_at: 1,
            bytes: data.len() as u64,
            files: vec![FileEntry {
                path: "region/r.0.0.mca".into(),
                size: data.len() as u64,
                chunks,
                region: None,
                modified: None,
            }],
            note: None,
            dirs: Vec::new(),
        };
        let dir = snapshots_dir(&game, "World");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("1-0.json"), serde_json::to_vec(&old).unwrap()).unwrap();

        let mut later = base.clone();
        later[0] = slot(0, 20, chunk(50, 500));
        write_region(&game, "World", "region", "r.0.0.mca", &later);
        let done = restore_chunks(&game, "World", "1-0", &Area::new("", (0, 0), (0, 0))).unwrap();
        assert_eq!(done.chunks, 1);
        assert_eq!(
            read_region(&game, "World", "region", "r.0.0.mca")[&0],
            chunk(0, 500)
        );
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn prune_keeps_latest_and_collects_garbage() {
        let game = temp_dir("prune");
        let mut slots: Vec<region::Slot> =
            (0..8).map(|i| slot(i, 1, chunk(i as u8, 2000))).collect();
        let mut ids = Vec::new();
        for round in 0..4u8 {
            slots[0] = slot(0, round as u32 + 2, chunk(100 + round, 2000));
            write_region(&game, "World", "region", "r.0.0.mca", &slots);
            ids.push(backup(&game, "World").unwrap().id);
        }
        assert_eq!(list(&game, "World").unwrap().len(), 4);
        let (removed, deleted) = prune(&game, 2).unwrap();
        assert_eq!(removed, 2);
        // 第二份快照的包里全是没用的对象，整个删掉；第一份的包大部分还有用，留着。
        assert!(deleted >= 2, "{deleted}");
        let remaining = list(&game, "World").unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].id, ids[3]);
        // 留下的两份都还能完整恢复。
        for snapshot in &remaining {
            fs::remove_dir_all(saves::saves_dir(&game).join("World")).ok();
            restore(&game, "World", &snapshot.id).unwrap();
        }
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn prune_keeps_snapshots_of_deleted_worlds() {
        let game = temp_dir("deleted-world");
        let slots: Vec<region::Slot> = (0..3).map(|i| slot(i, 1, chunk(i as u8, 2000))).collect();
        write_region(&game, "Gone", "region", "r.0.0.mca", &slots);
        let snap = backup(&game, "Gone").unwrap();
        // 世界删了，接着清理（自动备份结束时也会清理）。
        fs::remove_dir_all(saves::saves_dir(&game).join("Gone")).unwrap();
        let (_, deleted) = prune(&game, 5).unwrap();
        assert_eq!(deleted, 0);
        let target = restore(&game, "Gone", &snap.id).unwrap();
        assert_eq!(read_region(&game, "Gone", "region", "r.0.0.mca").len(), 3);
        assert!(target.ends_with("Gone"));
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn dimensions_are_found_and_named() {
        let snapshot = Snapshot {
            world: "W".into(),
            id: "1-0".into(),
            created_at: 0,
            bytes: 0,
            note: None,
            dirs: Vec::new(),
            files: [
                "region/r.0.0.mca",
                "DIM-1/region/r.0.0.mca",
                "DIM1/entities/r.0.0.mca",
                "dimensions/mod/sky/region/r.1.1.mca",
                "level.dat",
            ]
            .iter()
            .map(|p| FileEntry {
                path: p.to_string(),
                size: 0,
                chunks: vec![],
                region: None,
                modified: None,
            })
            .collect(),
        };
        let dims = dimensions(&snapshot);
        let labels: Vec<&str> = dims.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["主世界", "下界", "mod:sky"]);
        assert_eq!(dims[2].dir, "dimensions/mod/sky");
    }
}
