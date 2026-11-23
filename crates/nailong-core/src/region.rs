//! Anvil 区域文件（`r.<x>.<z>.mca`）：一个文件装 32×32 个区块。
//!
//! 前 4 KiB 是位置表（每个区块 4 字节：3 字节扇区偏移 + 1 字节扇区数），
//! 接着 4 KiB 是时间表（每个区块最后一次保存的秒数），之后按 4 KiB 扇区放区块：
//! 4 字节长度 + 1 字节压缩方式 + 数据。压缩方式带 0x80 时数据在旁边的 `c.<x>.<z>.mcc` 里。
//! `region/`、`entities/`、`poi/` 三个目录都是这个格式。

use crate::error::{Error, Result};
use std::path::Path;

pub const SECTOR: usize = 4096;
const HEADER: usize = 2 * SECTOR;

/// 一个区块：在文件里的序号（0..1024）、保存时间和原样的数据（压缩方式那 1 字节 + 压缩后的内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub index: u16,
    pub timestamp: u32,
    pub payload: Vec<u8>,
}

/// `r.-1.2.mca` → `(-1, 2)`。
pub fn parse_name(name: &str) -> Option<(i32, i32)> {
    let rest = name
        .strip_prefix("r.")?
        .strip_suffix(".mca")
        .or_else(|| name.strip_prefix("r.")?.strip_suffix(".mcr"))?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

pub fn file_name(rx: i32, rz: i32) -> String {
    format!("r.{rx}.{rz}.mca")
}

/// 区块坐标在区域文件里的序号。
pub fn index_of(cx: i32, cz: i32) -> u16 {
    ((cx & 31) + (cz & 31) * 32) as u16
}

/// 区块所在的区域。
pub fn region_of(cx: i32, cz: i32) -> (i32, i32) {
    (cx >> 5, cz >> 5)
}

/// 区域 `(rx, rz)` 里第 `index` 个区块的坐标。
pub fn chunk_at(rx: i32, rz: i32, index: u16) -> (i32, i32) {
    (rx * 32 + (index % 32) as i32, rz * 32 + (index / 32) as i32)
}

/// 太大放不进区域文件的区块存在旁边的这个文件里。
pub fn external_name(cx: i32, cz: i32) -> String {
    format!("c.{cx}.{cz}.mcc")
}

/// 这份数据是不是只是个标记，真正的内容在 `.mcc` 里。
pub fn is_external(payload: &[u8]) -> bool {
    payload.first().is_some_and(|b| b & 0x80 != 0)
}

/// 拆出所有区块。格式不对（被截断、位置越界）时为 None，调用方按普通文件处理。
pub fn parse(data: &[u8]) -> Option<Vec<Slot>> {
    if data.is_empty() {
        return Some(Vec::new());
    }
    if data.len() < HEADER {
        return None;
    }
    let mut out = Vec::new();
    for index in 0..1024usize {
        let loc = be32(data, index * 4);
        if loc == 0 {
            continue;
        }
        let offset = (loc >> 8) as usize;
        let count = (loc & 0xff) as usize;
        let start = offset * SECTOR;
        if offset < 2 || count == 0 || start + 5 > data.len() {
            return None;
        }
        let length = be32(data, start) as usize;
        if length == 0 || length + 4 > count * SECTOR || start + 4 + length > data.len() {
            return None;
        }
        out.push(Slot {
            index: index as u16,
            timestamp: be32(data, SECTOR + index * 4),
            payload: data[start + 4..start + 4 + length].to_vec(),
        });
    }
    Some(out)
}

/// 只读文件头：有哪些区块、各自的保存时间。文件不在或读不了时为空。
pub fn header(path: &Path) -> Vec<(u16, u32)> {
    use std::io::Read;
    let mut head = vec![0u8; HEADER];
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file.read_exact(&mut head).is_err() {
        return Vec::new();
    }
    (0..1024usize)
        .filter(|&i| be32(&head, i * 4) != 0)
        .map(|i| (i as u16, be32(&head, SECTOR + i * 4)))
        .collect()
}

/// 把区块按序号排好、紧挨着写成一个新的区域文件。
pub fn build(slots: &[Slot]) -> Result<Vec<u8>> {
    let mut sorted: Vec<&Slot> = slots.iter().collect();
    sorted.sort_by_key(|slot| slot.index);
    let mut out = vec![0u8; HEADER];
    for slot in sorted {
        if slot.index >= 1024 || slot.payload.is_empty() {
            return Err(Error::Plain("区块数据无效".into()));
        }
        let sectors = (4 + slot.payload.len()).div_ceil(SECTOR);
        if sectors > 255 {
            return Err(Error::Plain("区块太大，放不进区域文件".into()));
        }
        let offset = out.len() / SECTOR;
        let i = slot.index as usize;
        out[i * 4..i * 4 + 4]
            .copy_from_slice(&(((offset as u32) << 8) | sectors as u32).to_be_bytes());
        out[SECTOR + i * 4..SECTOR + i * 4 + 4].copy_from_slice(&slot.timestamp.to_be_bytes());
        out.extend_from_slice(&(slot.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&slot.payload);
        out.resize(offset * SECTOR + sectors * SECTOR, 0);
    }
    Ok(out)
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_coordinates() {
        assert_eq!(parse_name("r.-1.2.mca"), Some((-1, 2)));
        assert_eq!(parse_name("r.0.0.mcr"), Some((0, 0)));
        assert_eq!(parse_name("c.0.0.mcc"), None);
        assert_eq!(parse_name("r.a.0.mca"), None);
        assert_eq!(region_of(-1, 33), (-1, 1));
        let i = index_of(-1, 33);
        assert_eq!(chunk_at(-1, 1, i), (-1, 33));
    }

    #[test]
    fn build_then_parse_round_trips() {
        let slots = vec![
            Slot {
                index: 5,
                timestamp: 100,
                payload: vec![2; 10],
            },
            Slot {
                index: 1,
                timestamp: 7,
                payload: vec![2; 5000],
            },
        ];
        let data = build(&slots).unwrap();
        assert_eq!(data.len() % SECTOR, 0);
        let mut parsed = parse(&data).unwrap();
        parsed.sort_by_key(|s| s.index);
        assert_eq!(parsed[0], slots[1]);
        assert_eq!(parsed[1], slots[0]);
    }

    #[test]
    fn garbage_is_not_a_region() {
        assert!(parse(&vec![1u8; 20_000]).is_none());
        assert!(parse(&[0u8; 100]).is_none());
        assert_eq!(parse(&[]), Some(Vec::new()));
        assert_eq!(parse(&vec![0u8; HEADER]), Some(Vec::new()));
    }
}
