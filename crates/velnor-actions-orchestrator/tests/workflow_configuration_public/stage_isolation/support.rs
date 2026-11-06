// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

pub(super) use super::super::source_qualification::generator_repo;

pub(super) const GENERATE_LOCK_NAME: &str = ".github.velnor-generate.lock";

pub(super) const LEGACY_STAGE: &str = ".github.velnor-stage";
pub(super) const LEGACY_OWNER: &[u8] = b"unsupported-stage-format\nforeign-state\n";
pub(super) const USER_FILE: &[u8] = b"unmanaged repository content\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    kind: &'static str,
    device: u64,
    inode: u64,
    owner: u32,
    mode: u32,
    link_count: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    bytes: Vec<u8>,
}

pub(super) fn snapshot(root: &Path) -> Result<Vec<(String, Entry)>, Box<dyn Error>> {
    let mut entries = Vec::new();
    snapshot_into(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

pub(super) fn snapshot_content(root: &Path) -> Result<Vec<(String, Entry)>, Box<dyn Error>> {
    let mut entries = snapshot(root)?;
    for (_, entry) in &mut entries {
        entry.device = 0;
        entry.inode = 0;
        entry.owner = 0;
        entry.link_count = 0;
        entry.modified_seconds = 0;
        entry.modified_nanoseconds = 0;
    }
    Ok(entries)
}

pub(super) fn snapshot_preserving_children(
    root: &Path,
) -> Result<Vec<(String, Entry)>, Box<dyn Error>> {
    let mut entries = snapshot(root)?;
    let Some((_, root_entry)) = entries.iter_mut().find(|(path, _)| path.is_empty()) else {
        return Err(std::io::Error::other("root snapshot entry is missing").into());
    };
    root_entry.modified_seconds = 0;
    root_entry.modified_nanoseconds = 0;
    Ok(entries)
}

fn snapshot_into(
    root: &Path,
    path: &Path,
    entries: &mut Vec<(String, Entry)>,
) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
    let mode = metadata.mode() & 0o7777;
    let device = metadata.dev();
    let inode = metadata.ino();
    let owner = metadata.uid();
    let link_count = metadata.nlink();
    let modified_seconds = metadata.mtime();
    let modified_nanoseconds = metadata.mtime_nsec();
    if metadata.file_type().is_symlink() {
        entries.push((
            relative,
            Entry {
                kind: "symlink",
                device,
                inode,
                owner,
                mode,
                link_count,
                modified_seconds,
                modified_nanoseconds,
                bytes: fs::read_link(path)?.as_os_str().as_encoded_bytes().to_vec(),
            },
        ));
    } else if metadata.is_dir() {
        entries.push((
            relative,
            Entry {
                kind: "directory",
                device,
                inode,
                owner,
                mode,
                link_count,
                modified_seconds,
                modified_nanoseconds,
                bytes: Vec::new(),
            },
        ));
        for child in fs::read_dir(path)? {
            snapshot_into(root, &child?.path(), entries)?;
        }
    } else {
        entries.push((
            relative,
            Entry {
                kind: "file",
                device,
                inode,
                owner,
                mode,
                link_count,
                modified_seconds,
                modified_nanoseconds,
                bytes: fs::read(path)?,
            },
        ));
    }
    Ok(())
}

pub(super) fn generate_in_place(repo: &TempDir) -> Result<Vec<String>, Box<dyn Error>> {
    let preparation = prepare(repo.path())?;
    let report = generate(&preparation, &GenerateOptions::default())?;
    Ok(report.files_written)
}

pub(super) fn make_legacy_stage(repo: &TempDir, owner: &[u8]) -> Result<(), Box<dyn Error>> {
    let root = repo.path();
    let stage = root.join(LEGACY_STAGE);
    fs::create_dir(&stage)?;
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?;
    fs::create_dir(stage.join("spare"))?;
    fs::set_permissions(stage.join("spare"), fs::Permissions::from_mode(0o700))?;
    fs::write(stage.join("owner"), owner)?;
    fs::write(
        stage.join("spare/user-state.bin"),
        b"keep every byte\0\x91\n",
    )?;
    fs::write(root.join("legacy-sentinel"), b"outside stage sentinel\n")?;
    std::os::unix::fs::symlink(root.join("legacy-sentinel"), stage.join("spare/link"))?;
    Ok(())
}

pub(super) fn make_foreign_legacy_stage(repo: &TempDir) -> Result<(), Box<dyn Error>> {
    make_legacy_stage(
        repo,
        b"velnor-generate-stage-v1\n/foreign/repository\n999999\n888888\n",
    )
}

pub(super) fn make_symlink_owner_legacy_stage(repo: &TempDir) -> Result<(), Box<dyn Error>> {
    let root = repo.path();
    let stage = root.join(LEGACY_STAGE);
    fs::create_dir(&stage)?;
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?;
    fs::create_dir(stage.join("spare"))?;
    fs::set_permissions(stage.join("spare"), fs::Permissions::from_mode(0o700))?;
    let sentinel = root.join("owner-sentinel");
    fs::write(&sentinel, b"owner bytes remain private\n")?;
    std::os::unix::fs::symlink(&sentinel, stage.join("owner"))?;
    Ok(())
}

pub(super) fn assert_no_invocation_stage(repo: &TempDir) -> Result<(), Box<dyn Error>> {
    assert_no_invocation_stage_at(repo.path())
}

pub(super) fn assert_no_invocation_stage_at(root: &Path) -> Result<(), Box<dyn Error>> {
    let names = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        names
            .iter()
            .all(|name| { !name.to_string_lossy().starts_with(".github.velnor-stage-") }),
        "successful generation must remove its private invocation stage"
    );
    Ok(())
}

pub(super) fn distinct_checkout_paths(
    repo: &TempDir,
) -> Result<(PathBuf, PathBuf, RemoveOnDrop), Box<dyn Error>> {
    let parent = repo.path().parent().ok_or("temporary repo has no parent")?;
    let name = repo
        .path()
        .file_name()
        .ok_or("temporary repo has no name")?
        .to_string_lossy();
    let replacement = parent.join(format!("{name}-replacement"));
    let backup = parent.join(format!("{name}-original"));
    Ok((replacement.clone(), backup, RemoveOnDrop(replacement)))
}

pub(super) fn copy_checkout(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    copy_checkout_inner(source, destination, true)
}

fn copy_checkout_inner(
    source: &Path,
    destination: &Path,
    checkout_root: bool,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if checkout_root
            && (name == GENERATE_LOCK_NAME
                || name.to_string_lossy().starts_with(".github.velnor-stage-"))
        {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(name);
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            symlink(fs::read_link(source_path)?, destination_path)?;
        } else if metadata.is_dir() {
            copy_checkout_inner(&source_path, &destination_path, false)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
        } else {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
        }
    }
    Ok(())
}

pub(super) struct RemoveOnDrop(pub(super) PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}
