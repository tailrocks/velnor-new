// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha1_checked::{Digest as _, Sha1};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeSet,
    error::Error,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

fn invalid() -> Box<dyn Error> {
    "invalid captured fixture source".into()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

fn decode_hex(value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if value.is_empty()
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b - b'a' + 10
                }
            };
            Ok((digit(pair[0]) << 4) | digit(pair[1]))
        })
        .collect()
}

fn string<'a>(entry: &'a serde_json::Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    entry
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)
}

fn number(entry: &serde_json::Value, key: &str) -> Result<u64, Box<dyn Error>> {
    entry
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(invalid)
}

#[cfg(unix)]
fn read_entry(root: &Path, raw: &[u8], symlink: bool) -> Result<(Vec<u8>, u32), Box<dyn Error>> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, open, openat, readlinkat, statat};
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = open(root, flags | OFlags::DIRECTORY, Mode::empty())?;
    let parts = raw.split(|byte| *byte == b'/').collect::<Vec<_>>();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == b"." || *part == b".." || part.contains(&0))
    {
        return Err(invalid());
    }
    for part in &parts[..parts.len() - 1] {
        directory = openat(
            &directory,
            OsStr::from_bytes(part),
            flags | OFlags::DIRECTORY,
            Mode::empty(),
        )?;
    }
    let name = OsStr::from_bytes(parts[parts.len() - 1]);
    if symlink {
        let before = statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(before.st_mode) != FileType::Symlink {
            return Err(invalid());
        }
        let target = readlinkat(&directory, name, Vec::new())?.into_bytes();
        let after = statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if before.st_ino != after.st_ino
            || before.st_dev != after.st_dev
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || target.len() > 4096
        {
            return Err(invalid());
        }
        return Ok((target, 0));
    }
    let file = openat(&directory, name, flags, Mode::empty())?;
    let before = fstat(&file)?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile || before.st_nlink != 1 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    let mut file = std::fs::File::from(file);
    (&mut file)
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)?;
    let after = fstat(&file)?;
    if bytes.len() > MAX_FILE
        || u64::try_from(bytes.len())? != u64::try_from(before.st_size)?
        || before.st_size != after.st_size
        || before.st_ino != after.st_ino
        || before.st_dev != after.st_dev
        || before.st_mode != after.st_mode
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
    {
        return Err(invalid());
    }
    #[cfg(target_os = "macos")]
    let mode = u32::from(before.st_mode & 0o777);
    #[cfg(not(target_os = "macos"))]
    let mode = before.st_mode & 0o777;
    Ok((bytes, mode))
}

#[cfg(not(unix))]
fn read_entry(_root: &Path, _raw: &[u8], _symlink: bool) -> Result<(Vec<u8>, u32), Box<dyn Error>> {
    Err(invalid())
}

#[cfg(unix)]
fn verify_gitlink(root: &Path, raw: &[u8]) -> Result<(), Box<dyn Error>> {
    use rustix::fs::{Dir, Mode, OFlags, open, openat};
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::DIRECTORY;
    let mut directory = open(root, flags, Mode::empty())?;
    for part in raw.split(|byte| *byte == b'/') {
        if part.is_empty() || part == b"." || part == b".." || part.contains(&0) {
            return Err(invalid());
        }
        directory = openat(&directory, OsStr::from_bytes(part), flags, Mode::empty())?;
    }
    let mut entries = Dir::read_from(&directory)?;
    while let Some(entry) = entries.read() {
        if !matches!(entry?.file_name().to_bytes(), b"." | b"..") {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn verify_capture(root: &Path, manifest: &str) -> Result<(), Box<dyn Error>> {
    use std::os::unix::ffi::OsStringExt;
    verify_git_custody(root)?;
    let value = velnor_actions_contract::parse_strict_json(manifest)?;
    let object = value.as_object().ok_or_else(invalid)?;
    if object.len() != 3 || number(&value, "schema")? != 1 {
        return Err(invalid());
    }
    let entries = value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?;
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err(invalid());
    }
    let mut paths = BTreeSet::new();
    let mut index = Vec::new();
    let mut total = 0usize;
    let mut previous: Option<Vec<u8>> = None;
    for entry in entries {
        if entry.as_object().is_none_or(|v| v.len() != 8) {
            return Err(invalid());
        }
        let path = decode_hex(string(entry, "path_hex")?)?;
        if path.len() > 1024
            || path
                .split(|b| *b == b'/')
                .any(|part| part.eq_ignore_ascii_case(b".git"))
            || previous.as_ref().is_some_and(|prior| prior >= &path)
        {
            return Err(invalid());
        }
        previous = Some(path.clone());
        let mode = string(entry, "mode")?;
        if !matches!(mode, "100644" | "100755" | "120000" | "160000") {
            return Err(invalid());
        }
        let oid = string(entry, "oid")?;
        if oid.len() != 40 || decode_hex(oid)?.len() != 20 {
            return Err(invalid());
        }
        let skip = entry
            .get("skip")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(invalid)?;
        let hydrated = entry
            .get("hydrated")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(invalid)?;
        if hydrated && !skip {
            return Err(invalid());
        }
        let (bytes, physical_mode) = if mode == "160000" {
            verify_gitlink(root, &path)?;
            (Vec::new(), 0)
        } else {
            read_entry(root, &path, mode == "120000")?
        };
        total = total.checked_add(bytes.len()).ok_or_else(invalid)?;
        if total > MAX_TOTAL
            || number(entry, "size")? != u64::try_from(bytes.len())?
            || number(entry, "physical_mode")? != u64::from(physical_mode)
            || string(entry, "sha256")? != hex(&Sha256::digest(&bytes))
        {
            return Err(invalid());
        }
        if hydrated && mode != "160000" {
            verify_blob(&bytes, oid)?;
        }
        index.extend_from_slice(format!("{mode} {oid} 0\t").as_bytes());
        index.extend_from_slice(&path);
        index.push(0);
        paths.insert(PathBuf::from(OsString::from_vec(path)));
    }
    if string(&value, "index_sha256")? != hex(&Sha256::digest(&index)) {
        return Err(invalid());
    }
    verify_index(root, &index)?;
    verify_membership(root, &paths)
}

#[cfg(unix)]
fn verify_blob(bytes: &[u8], oid: &str) -> Result<(), Box<dyn Error>> {
    let mut digest = Sha1::new();
    digest.update(format!("blob {}\0", bytes.len()).as_bytes());
    digest.update(bytes);
    let result = digest.try_finalize();
    if result.has_collision() || hex(result.hash()) != oid {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(unix)]
fn verify_index(root: &Path, index: &[u8]) -> Result<(), Box<dyn Error>> {
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))?;
    let mut command = std::process::Command::new("/usr/bin/git");
    command
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.ignorecase=false",
            "ls-files",
            "--stage",
            "-z",
        ]);
    let (actual, _) = velnor_actions_mise::command::run_prepared_quiescent_until(
        &mut command,
        64 * 1024 * 1024,
        deadline,
    )?;
    if !actual.success || actual.stdout != index {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(unix)]
fn verify_git_custody(root: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::MetadataExt;
    for relative in [".git", ".git/objects", ".git/objects/info"] {
        let metadata = std::fs::symlink_metadata(root.join(relative))?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid());
        }
    }
    for relative in [
        ".git/objects/info/alternates",
        ".git/objects/info/http-alternates",
    ] {
        match std::fs::symlink_metadata(root.join(relative)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn verify_capture(_root: &Path, _manifest: &str) -> Result<(), Box<dyn Error>> {
    Err(invalid())
}

fn verify_membership(root: &Path, paths: &BTreeSet<PathBuf>) -> Result<(), Box<dyn Error>> {
    let mut expected = paths.clone();
    for path in paths {
        let mut parent = path.parent();
        while let Some(dir) = parent.filter(|dir| !dir.as_os_str().is_empty()) {
            expected.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    let mut found = BTreeSet::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(root.join(&dir))? {
            let entry = entry?;
            let path = dir.join(entry.file_name());
            if path == Path::new(".git") {
                continue;
            }
            let metadata = std::fs::symlink_metadata(root.join(&path))?;
            if metadata.is_dir() {
                pending.push(path.clone());
            } else if !metadata.is_file() && !metadata.file_type().is_symlink() {
                return Err(invalid());
            }
            if !found.insert(path) {
                return Err(invalid());
            }
        }
    }
    if found != expected {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "capture_verify_tests.rs"]
mod tests;
