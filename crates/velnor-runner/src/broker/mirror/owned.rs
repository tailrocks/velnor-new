// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{DirEntry, FileType};
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};

const OBJECT_TEMPORARIES: [&[u8]; 1] = [b"tmp_obj_"];
const PACK_TEMPORARIES: [&[u8]; 2] = [b"tmp_pack_", b"tmp_idx_"];

fn entries(dir: &Path) -> Result<Vec<DirEntry>> {
    match std::fs::read_dir(dir) {
        Ok(listing) => listing
            .collect::<std::io::Result<_>>()
            .with_context(|| format!("read {}", dir.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", dir.display())),
    }
}

fn remove(entry: &DirEntry, kind: FileType) -> Result<()> {
    let path = entry.path();
    let removed = if kind.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    match removed {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        other => other.with_context(|| format!("remove {}", path.display())),
    }
}

fn retain(dir: &Path, owns: impl Fn(&DirEntry, &[u8], FileType) -> bool) -> Result<()> {
    for entry in entries(dir)? {
        let kind = entry
            .file_type()
            .with_context(|| format!("inspect {}", entry.path().display()))?;
        if !owns(&entry, entry.file_name().as_bytes(), kind) {
            remove(&entry, kind)?;
        }
    }
    Ok(())
}

fn is_fan_out(name: &[u8]) -> bool {
    matches!(name, [b'0'..=b'9' | b'a'..=b'f', b'0'..=b'9' | b'a'..=b'f'])
}

fn is_temporary(name: &[u8], prefixes: &[&[u8]]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn predates(entry: &DirEntry, acquired: SystemTime) -> bool {
    entry
        .metadata()
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| modified < acquired)
}

fn stem_of(name: &[u8]) -> Option<&[u8]> {
    let end = name.iter().position(|byte| *byte == b'.')?;
    let stem = &name[..end];
    stem.starts_with(b"pack-").then_some(stem)
}

fn indexed_stems(pack: &Path) -> Result<BTreeSet<Vec<u8>>> {
    let mut stems = BTreeSet::new();
    for entry in entries(pack)? {
        let name = entry.file_name();
        if let Some(stem) = stem_of(name.as_bytes()).filter(|_| name.as_bytes().ends_with(b".idx"))
        {
            stems.insert(stem.to_vec());
        }
    }
    Ok(stems)
}

fn tidy_top(mirror: &Path) -> Result<()> {
    retain(mirror, |_, name, kind| match name {
        b"HEAD" | b"config" | b"packed-refs" => kind.is_file(),
        b"objects" | b"refs" | b"quarantine" | b"sound" => kind.is_dir(),
        _ => false,
    })
}

fn tidy_objects(mirror: &Path, acquired: SystemTime) -> Result<()> {
    let objects = mirror.join("objects");
    retain(&objects, |entry, name, kind| match name {
        b"pack" => kind.is_dir(),
        _ if is_fan_out(name) => kind.is_dir(),
        _ => is_temporary(name, &OBJECT_TEMPORARIES) && !predates(entry, acquired),
    })?;
    for fan_out in entries(&objects)? {
        if is_fan_out(fan_out.file_name().as_bytes()) {
            retain(&fan_out.path(), |entry, name, _| {
                !(is_temporary(name, &OBJECT_TEMPORARIES) && predates(entry, acquired))
            })?;
        }
    }
    let pack = objects.join("pack");
    match std::fs::create_dir(&pack) {
        Err(error) if error.kind() != ErrorKind::AlreadyExists => {
            return Err(error).with_context(|| format!("create {}", pack.display()));
        }
        Ok(()) | Err(_) => {}
    }
    let indexed = indexed_stems(&pack)?;
    retain(&pack, |entry, name, kind| {
        if is_temporary(name, &PACK_TEMPORARIES) {
            !predates(entry, acquired)
        } else {
            kind.is_file() && stem_of(name).is_some_and(|stem| indexed.contains(stem))
        }
    })
}

pub(super) async fn keep_only_owned(mirror: &Path) -> Result<()> {
    let mirror = mirror.to_owned();
    tokio::task::spawn_blocking(move || tidy_top(&mirror))
        .await
        .context("wait for the mirror tidy")?
}

pub(super) async fn sweep_objects(mirror: &Path, acquired: SystemTime) -> Result<()> {
    let mirror = mirror.to_owned();
    tokio::task::spawn_blocking(move || tidy_objects(&mirror, acquired))
        .await
        .context("wait for the object store sweep")?
}
