// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::native_cache_support::Store;

pub(crate) fn tar_member(name: &[u8], kind: u8, link: &[u8], content: &[u8]) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    header.as_old_mut().name[..name.len()].copy_from_slice(name);
    header.as_old_mut().linkname[..link.len()].copy_from_slice(link);
    header.set_entry_type(tar::EntryType::new(kind));
    header.set_mode(if kind == b'5' { 0o755 } else { 0o644 });
    header.set_size(content.len() as u64);
    header.set_cksum();
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend_from_slice(content);
    bytes.resize(bytes.len().div_ceil(512) * 512, 0);
    bytes
}

pub(crate) fn unlock_entry(store: &Store) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = store.entry("k-1")?.dir;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(dir.join("data.tar"), fs::Permissions::from_mode(0o644))?;
    Ok(dir)
}

pub(crate) fn overwrite_entry(store: &Store, members: &[Vec<u8>]) -> Result<()> {
    let mut archive: Vec<u8> = members.concat();
    archive.extend_from_slice(&[0; 1024]);
    fs::write(unlock_entry(store)?.join("data.tar"), archive)?;
    Ok(())
}
