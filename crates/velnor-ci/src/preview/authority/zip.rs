// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use miniz_oxide::inflate::decompress_to_vec_with_limit;

use crate::error::CiError;
use crate::preview::json::{Res, require};

const END: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;

fn u16_at(data: &[u8], at: usize) -> Res<u32> {
    data.get(at..at + 2)
        .map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])))
        .ok_or_else(|| CiError::new("source preview archive"))
}

fn u32_at(data: &[u8], at: usize) -> Res<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| CiError::new("source preview archive"))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

struct Entry {
    name: String,
    flags: u32,
    method: u32,
    crc: u32,
    compressed: usize,
    size: usize,
    mode: u32,
    offset: usize,
}

fn end_of_directory(data: &[u8]) -> Res<(usize, usize)> {
    require(data.len() >= 22)?;
    let start = data.len().saturating_sub(22 + 65_535);
    let at = (start..=data.len() - 22)
        .rev()
        .find(|&i| u32_at(data, i).is_ok_and(|sig| sig == END))
        .ok_or_else(|| CiError::new("source preview archive"))?;
    let count = u16_at(data, at + 10)? as usize;
    require(u16_at(data, at + 4)? == 0 && u16_at(data, at + 6)? == 0)?;
    require(u16_at(data, at + 8)? as usize == count && u16_at(data, at + 20)? == 0)?;
    let size = u32_at(data, at + 12)? as usize;
    let offset = u32_at(data, at + 16)? as usize;
    require(count != 0xFFFF && size != 0xFFFF_FFFF && offset != 0xFFFF_FFFF)?;
    require(offset.checked_add(size) == Some(at))?;
    Ok((count, offset))
}

fn entries(data: &[u8]) -> Res<Vec<Entry>> {
    let (count, mut at) = end_of_directory(data)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        require(u32_at(data, at)? == CENTRAL)?;
        let name_len = u16_at(data, at + 28)? as usize;
        let extra = u16_at(data, at + 30)? as usize;
        let comment = u16_at(data, at + 32)? as usize;
        let name = data
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| CiError::new("source preview archive"))?;
        let compressed = u32_at(data, at + 20)?;
        let size = u32_at(data, at + 24)?;
        let offset = u32_at(data, at + 42)?;
        require(compressed != 0xFFFF_FFFF && size != 0xFFFF_FFFF && offset != 0xFFFF_FFFF)?;
        result.push(Entry {
            name: String::from_utf8(name.to_vec())
                .map_err(|_| CiError::new("source preview archive"))?,
            flags: u16_at(data, at + 8)?,
            method: u16_at(data, at + 10)?,
            crc: u32_at(data, at + 16)?,
            compressed: compressed as usize,
            size: size as usize,
            mode: u32_at(data, at + 38)? >> 16,
            offset: offset as usize,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(result)
}

fn read(data: &[u8], entry: &Entry) -> Res<Vec<u8>> {
    require(u32_at(data, entry.offset)? == LOCAL)?;
    let name_len = u16_at(data, entry.offset + 26)? as usize;
    let extra = u16_at(data, entry.offset + 28)? as usize;
    let name = data.get(entry.offset + 30..entry.offset + 30 + name_len);
    require(name == Some(entry.name.as_bytes()))?;
    let start = entry.offset + 30 + name_len + extra;
    let raw = data
        .get(start..start + entry.compressed)
        .ok_or_else(|| CiError::new("source preview archive"))?;
    let body = if entry.method == 0 {
        raw.to_vec()
    } else {
        decompress_to_vec_with_limit(raw, entry.size)
            .map_err(|_| CiError::new("source preview archive"))?
    };
    require(body.len() == entry.size && crc32(&body) == entry.crc)?;
    Ok(body)
}

/// Port of the archive half of `artifact_files`: the archive holds exactly the files in
/// `expected` (name to largest size), each a plain stored or deflated, unencrypted,
/// non-empty regular file within its limit. Returns name to content.
pub fn files(data: &[u8], expected: &BTreeMap<&str, usize>) -> Res<BTreeMap<String, Vec<u8>>> {
    let list = entries(data)?;
    let mut names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    require(list.len() == expected.len() && names.len() == expected.len())?;
    require(names.iter().all(|n| expected.contains_key(n)))?;
    let mut result = BTreeMap::new();
    for entry in &list {
        require(entry.flags & 1 == 0 && entry.mode & S_IFMT == S_IFREG)?;
        require(entry.method == 0 || entry.method == 8)?;
        let limit = expected.get(entry.name.as_str()).copied().unwrap_or(0);
        require(entry.size > 0 && entry.size <= limit)?;
        result.insert(entry.name.clone(), read(data, entry)?);
    }
    Ok(result)
}
