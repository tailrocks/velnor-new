// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use anyhow::Result;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub(crate) fn zip_with_mode(name: &str, bytes: &[u8], mode: u32) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file(name, SimpleFileOptions::default().unix_permissions(mode))?;
    writer.write_all(bytes)?;
    Ok(writer.finish()?.into_inner())
}

pub(crate) fn announcing_sizes(mut zip: Vec<u8>, size: u32) -> Vec<u8> {
    const CENTRAL: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    const SIZE_AT: usize = 24;
    let starts: Vec<usize> = zip
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == CENTRAL)
        .map(|(at, _)| at)
        .collect();
    for at in starts {
        zip[at + SIZE_AT..at + SIZE_AT + 4].copy_from_slice(&size.to_le_bytes());
    }
    zip
}

pub(crate) fn zip_with_symlink(name: &str, target: &str) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_symlink(name, target, SimpleFileOptions::default())?;
    Ok(writer.finish()?.into_inner())
}

pub(crate) fn forged_zip(entries: u64) -> Vec<u8> {
    const HEADER_BYTES: u64 = 46;
    let directory = HEADER_BYTES * entries;
    let mut bytes = vec![0_u8; usize::try_from(directory).unwrap_or_default()];
    let record_offset = directory;
    bytes.extend_from_slice(&0x0606_4b50_u32.to_le_bytes());
    bytes.extend_from_slice(&44_u64.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&entries.to_le_bytes());
    bytes.extend_from_slice(&entries.to_le_bytes());
    bytes.extend_from_slice(&directory.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0x0706_4b50_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&record_offset.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes
}
