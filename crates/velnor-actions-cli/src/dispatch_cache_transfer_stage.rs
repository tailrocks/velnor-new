use super::super::manifest::{PayloadManifest, PayloadRoot};
use super::super::roots::{CacheRootObservation, CacheRootSet};
use super::cleanup::remove_created_at;
use super::dirs::{
    c_name, open_directory_at, open_owned_directory, same_object, same_version, validate_file_stat,
    validate_owned_directory,
};
use super::engine::{snapshot_root, useful_state_digest};
use super::files::copy_file_to_new;
use super::payload;
use super::{
    MANIFEST_FILENAME, MAX_ENTRIES, MAX_MANIFEST_BYTES, ROOTS_DIRECTORY, is_file_root, root_name,
};
use rustix::fs::{AtFlags, Dir, Mode, OFlags, Stat, openat, statat};
use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

pub(super) fn write_new_payload(
    stage_path: &Path,
    runner_temp: &Path,
    roots: &CacheRootSet,
    manifest_text: &str,
) -> Result<CreatedStage, &'static str> {
    let (parent, stage_name, stage_fd, stage_stat) =
        create_stage_directory(runner_temp, stage_path)?;
    let created = CreatedStage {
        parent,
        name: stage_name,
        descriptor: stage_fd,
        stat: stage_stat,
    };
    if let Err(error) = populate_stage(&created.descriptor, stage_path, roots, manifest_text) {
        created.remove()?;
        return Err(error);
    }
    Ok(created)
}

pub(super) struct CreatedStage {
    parent: rustix::fd::OwnedFd,
    name: CString,
    descriptor: rustix::fd::OwnedFd,
    stat: Stat,
}

impl CreatedStage {
    pub(super) fn remove(&self) -> Result<(), &'static str> {
        remove_created_at(
            &self.parent,
            self.name.as_c_str(),
            &self.descriptor,
            &self.stat,
        )
    }
}

fn populate_stage(
    stage_fd: &rustix::fd::OwnedFd,
    stage_path: &Path,
    roots: &CacheRootSet,
    manifest_text: &str,
) -> Result<(), &'static str> {
    let roots_name = stage_name(ROOTS_DIRECTORY)?;
    rustix::fs::mkdirat(
        stage_fd,
        roots_name.as_c_str(),
        Mode::RUSR | Mode::WUSR | Mode::XUSR,
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let root_fd = open_directory_at(stage_fd, roots_name.as_c_str())?;
    validate_owned_directory(&root_fd)?;
    let roots_path = stage_path.join(ROOTS_DIRECTORY);
    for root in &roots.roots {
        let snapshot = snapshot_root(root)?;
        if !snapshot.present {
            continue;
        }
        let destination = roots_path.join(root_name(root.id));
        let copied = if is_file_root(root.id) {
            copy_file_to_new(&root.path, &destination)?
        } else {
            payload::copy_tree_to_new_dir(&root.path, &destination)?
        };
        if snapshot.digest.as_deref() != Some(copied.as_str()) {
            return Err("qualification_cache_payload_copy_mismatch");
        }
    }
    write_manifest_at(stage_fd, manifest_text)?;
    Ok(())
}

pub(super) fn verify_staged_payload(
    stage_path: &Path,
    binding: &QualificationCacheBindingOutputs,
    roots: &CacheRootSet,
    cache_key: &str,
    restore: bool,
) -> Result<PayloadManifest, &'static str> {
    let stage_fd = open_owned_directory(stage_path)?;
    let expected_top = BTreeSet::from([MANIFEST_FILENAME.to_owned(), ROOTS_DIRECTORY.to_owned()]);
    if read_names(&stage_fd)? != expected_top {
        return Err("qualification_cache_payload_layout_invalid");
    }
    let manifest_text = read_manifest_at(&stage_fd)?;
    let manifest = if restore {
        PayloadManifest::parse_for_restore(&manifest_text, binding, roots, cache_key)?
    } else {
        PayloadManifest::parse_for_save(&manifest_text, binding, roots, cache_key)?
    };
    let roots_name = stage_name(ROOTS_DIRECTORY)?;
    let roots_fd = open_directory_at(&stage_fd, roots_name.as_c_str())?;
    validate_owned_directory(&roots_fd)?;
    let expected_names = manifest
        .roots
        .iter()
        .filter(|root| root.present)
        .map(|root| root_name(root.id).to_owned())
        .collect::<BTreeSet<_>>();
    if read_names(&roots_fd)? != expected_names {
        return Err("qualification_cache_payload_layout_invalid");
    }
    for expected in &manifest.roots {
        let path = PathBuf::from(stage_path)
            .join(ROOTS_DIRECTORY)
            .join(root_name(expected.id));
        let actual = if expected.present {
            snapshot_root(&CacheRootObservation {
                id: expected.id,
                path,
            })?
        } else {
            PayloadRoot {
                id: expected.id,
                present: false,
                mode: None,
                digest: None,
            }
        };
        if actual != *expected {
            return Err("qualification_cache_payload_digest_mismatch");
        }
    }
    if useful_state_digest(&manifest.roots)? != manifest.state_digest {
        return Err("qualification_cache_payload_state_mismatch");
    }
    Ok(manifest)
}

fn write_manifest_at(stage_fd: &rustix::fd::OwnedFd, text: &str) -> Result<(), &'static str> {
    if text.len() > MAX_MANIFEST_BYTES || text.contains(['\n', '\r']) {
        return Err("qualification_cache_manifest_limit");
    }
    let name = stage_name(MANIFEST_FILENAME)?;
    let fd = openat(
        stage_fd,
        name.as_c_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| "qualification_cache_manifest_write_failed")?;
    let mut file = fs::File::from(fd);
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|_| "qualification_cache_manifest_write_failed")
}

fn read_manifest_at(stage_fd: &rustix::fd::OwnedFd) -> Result<String, &'static str> {
    let name = stage_name(MANIFEST_FILENAME)?;
    let fd = openat(
        stage_fd,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_manifest_read_failed")?;
    let before = rustix::fs::fstat(&fd).map_err(|_| "qualification_cache_manifest_read_failed")?;
    validate_file_stat(&before)?;
    let size = usize::try_from(before.st_size).map_err(|_| "qualification_cache_manifest_limit")?;
    if size > MAX_MANIFEST_BYTES {
        return Err("qualification_cache_manifest_limit");
    }
    let mut file = fs::File::from(fd);
    let mut bytes = Vec::with_capacity(size);
    (&mut file)
        .take((MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "qualification_cache_manifest_read_failed")?;
    let after = rustix::fs::fstat(&file).map_err(|_| "qualification_cache_manifest_read_failed")?;
    if bytes.len() != size || !same_version(&before, &after) {
        return Err("qualification_cache_manifest_changed");
    }
    String::from_utf8(bytes).map_err(|_| "qualification_cache_manifest_invalid")
}

fn create_stage_directory(
    runner_temp: &Path,
    stage_path: &Path,
) -> Result<(rustix::fd::OwnedFd, CString, rustix::fd::OwnedFd, Stat), &'static str> {
    let relative = stage_path
        .strip_prefix(runner_temp)
        .map_err(|_| "qualification_cache_stage_path_invalid")?;
    let mut components = relative.components().peekable();
    let mut parent = open_owned_directory(runner_temp)?;
    let mut final_name = None;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err("qualification_cache_stage_path_invalid");
        };
        let name = c_name(name)?;
        if components.peek().is_some() {
            parent = ensure_child_directory(&parent, name.as_c_str())?;
        } else {
            final_name = Some(name);
        }
    }
    let final_name = final_name.ok_or("qualification_cache_stage_path_invalid")?;
    rustix::fs::mkdirat(
        &parent,
        final_name.as_c_str(),
        Mode::RUSR | Mode::WUSR | Mode::XUSR,
    )
    .map_err(|_| "qualification_cache_stage_exists")?;
    let stat = statat(&parent, final_name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_stage_open_failed")?;
    let stage_fd = open_directory_at(&parent, final_name.as_c_str())?;
    validate_owned_directory(&stage_fd)?;
    if !same_object(
        &stat,
        &rustix::fs::fstat(&stage_fd).map_err(|_| "qualification_cache_stage_open_failed")?,
    ) {
        remove_created_at(&parent, final_name.as_c_str(), &stage_fd, &stat)?;
        return Err("qualification_cache_stage_changed");
    }
    Ok((parent, final_name, stage_fd, stat))
}

pub(super) fn ensure_child_directory(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
) -> Result<rustix::fd::OwnedFd, &'static str> {
    if let Ok(fd) = open_directory_at(parent, name) {
        validate_owned_directory(&fd)?;
        return Ok(fd);
    }
    rustix::fs::mkdirat(parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR)
        .map_err(|_| "qualification_cache_stage_directory_invalid")?;
    let fd = open_directory_at(parent, name)?;
    validate_owned_directory(&fd)?;
    Ok(fd)
}

pub(super) fn read_names(
    directory: &rustix::fd::OwnedFd,
) -> Result<BTreeSet<String>, &'static str> {
    let entries =
        Dir::read_from(directory).map_err(|_| "qualification_cache_directory_read_failed")?;
    let mut names = BTreeSet::new();
    for entry in entries {
        if names.len() >= MAX_ENTRIES {
            return Err("qualification_cache_payload_limit");
        }
        let name = entry
            .map_err(|_| "qualification_cache_directory_read_failed")?
            .file_name()
            .to_str()
            .map_err(|_| "qualification_cache_payload_path_invalid")?
            .to_owned();
        if name != "." && name != ".." && !names.insert(name) {
            return Err("qualification_cache_payload_layout_invalid");
        }
    }
    Ok(names)
}

fn stage_name(value: &str) -> Result<CString, &'static str> {
    CString::new(value).map_err(|_| "qualification_cache_payload_path_invalid")
}
