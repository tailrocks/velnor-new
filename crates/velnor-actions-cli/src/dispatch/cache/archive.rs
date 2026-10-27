// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_orchestrator::cache_recovery::sha256_hex;

use super::{MAX_ARCHIVE_MEMBERS, STATE_BASE};

/// Screen one archive member name against the state family allowlist.
///
/// Only the state root family may land: the bare base entry itself or
/// paths beneath it, with no absolute paths and no `.`/`..` components.
/// A hostile `tools/sentinel` member, an absolute path, or a traversal
/// attempt fails closed here, before anything is written.
fn member_allowed(member: &str, base: &str) -> bool {
    let member = member.strip_prefix("./").unwrap_or(member);
    let member = member.strip_suffix('/').unwrap_or(member);
    if member.is_empty() {
        return false;
    }
    if member.starts_with('/') {
        return false;
    }
    let mut parts = member.split('/');
    if parts.next() != Some(base) {
        return false;
    }
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
    }
    true
}

pub(super) fn list_archive_members(payload: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("tar")
        .arg("-tzf")
        .arg(payload)
        .output()
        .map_err(|_| "cache_tar_unavailable".to_owned())?;
    if !output.status.success() {
        return Err("cache_payload_unreadable".to_owned());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(str::to_owned).collect())
}

pub(super) fn screen_archive_members(members: &[String]) -> Result<(), String> {
    if u64::try_from(members.len()).unwrap_or(u64::MAX) > MAX_ARCHIVE_MEMBERS {
        return Err("cache_refused_shadow: member_over_budget".to_owned());
    }
    for member in members {
        if !member_allowed(member, STATE_BASE) {
            return Err(format!("cache_refused_shadow: {member}"));
        }
    }
    Ok(())
}

pub(super) fn payload_bytes(payload: &Path) -> Result<Vec<u8>, String> {
    match fs::symlink_metadata(payload) {
        Ok(meta) if !meta.file_type().is_symlink() && meta.is_file() => {}
        Ok(_) => return Err("cache_payload_not_regular".to_owned()),
        Err(_) => return Err("cache_payload_missing".to_owned()),
    }
    let bytes = fs::read(payload).map_err(|_| "cache_payload_unreadable".to_owned())?;
    if bytes.is_empty() {
        return Err("cache_payload_empty".to_owned());
    }
    Ok(bytes)
}

pub(super) fn reread_payload(
    payload: &Path,
    first: &[u8],
    op: &str,
) -> Result<(Vec<u8>, String), String> {
    let digest_one = sha256_hex(first);
    let Ok(reread) = fs::read(payload) else {
        return Err("cache_payload_unreadable".to_owned());
    };
    if reread.len() != first.len() || sha256_hex(&reread) != digest_one {
        return Err(format!("cache {op} readback_unstable"));
    }
    Ok((reread, digest_one))
}
