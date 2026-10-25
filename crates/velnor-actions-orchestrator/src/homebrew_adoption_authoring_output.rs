use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use velnor_actions_contract::homebrew_adoption::HomebrewAdoptionFileIdentity;

use super::HomebrewAdoptionAuthoringReport;
use crate::{OrchestratorError, internal::internal};

pub(super) const DELETION_MANIFEST_PATH: &str = ".velnor-homebrew-adoption-deletions.json";

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeletionManifest<'a> {
    pub(super) schema: &'static str,
    pub(super) transition_id: &'a str,
    pub(super) base_sha: &'a str,
    pub(super) base_tree: &'a str,
    pub(super) deleted_files: &'a [HomebrewAdoptionFileIdentity],
}

pub(super) fn fresh_output_path(
    output_dir: &Path,
    repository_root: &Path,
) -> Result<(PathBuf, PathBuf), OrchestratorError> {
    if !output_dir.is_absolute()
        || output_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(internal("homebrew_adoption_output_path_invalid"));
    }
    let name = output_dir
        .file_name()
        .filter(|name| *name != "." && *name != "..")
        .ok_or_else(|| internal("homebrew_adoption_output_path_invalid"))?;
    let parent = output_dir
        .parent()
        .ok_or_else(|| internal("homebrew_adoption_output_path_invalid"))?
        .canonicalize()
        .map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?;
    let root = repository_root
        .canonicalize()
        .map_err(|_| internal("homebrew_adoption_repository_root_invalid"))?;
    let target = parent.join(name);
    if target.starts_with(root) || fs::symlink_metadata(&target).is_ok() {
        return Err(internal("homebrew_adoption_output_not_fresh_external"));
    }
    Ok((parent, target))
}

pub(super) fn write_overlay(
    parent: &Path,
    target: &Path,
    transition_id: &str,
    base_sha: &str,
    base_tree: &str,
    files: &BTreeMap<String, (u32, Vec<u8>)>,
    deleted_files: Vec<HomebrewAdoptionFileIdentity>,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    if target.parent() != Some(parent)
        || target
            .file_name()
            .is_none_or(|name| name.to_str().is_none_or(str::is_empty))
    {
        return Err(internal("homebrew_adoption_output_target_invalid"));
    }
    let parent_fd = open_trusted_parent(parent)?;
    let mut stage_builder = tempfile::Builder::new();
    stage_builder.prefix(".velnor-homebrew-adoption-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        stage_builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let stage = stage_builder
        .tempdir_in(parent)
        .map_err(|_| internal("homebrew_adoption_output_stage"))?;
    let stage_name = stage
        .path()
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| internal("homebrew_adoption_output_stage"))?
        .to_owned();
    let stage_fd = open_stage_directory(&parent_fd, &stage_name)?;
    let stage_identity = directory_identity(&stage_fd)?;
    let mut written_paths = Vec::with_capacity(files.len() + 1);
    for (path, (mode, bytes)) in files {
        write_file(stage.path(), path, *mode, bytes)?;
        written_paths.push(path.clone());
    }
    let manifest = DeletionManifest {
        schema: "velnor.homebrew-adoption-deletions.v1",
        transition_id,
        base_sha,
        base_tree,
        deleted_files: &deleted_files,
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&manifest)
        .map_err(crate::internal::internal_contract)?;
    write_file(stage.path(), DELETION_MANIFEST_PATH, 0o100_644, &bytes)?;
    written_paths.push(DELETION_MANIFEST_PATH.to_owned());
    verify_stage_identity(&parent_fd, &stage_name, &stage_fd, stage_identity)?;
    let retained_stage = stage.keep();
    publish_staged_directory(
        &parent_fd,
        &stage_name,
        target
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| internal("homebrew_adoption_output_publish"))?,
        &stage_fd,
        stage_identity,
    )
    .map_err(|error| {
        internal(&format!(
            "{error}; retained_stage:{}",
            retained_stage.display()
        ))
    })?;
    written_paths.sort();
    let deleted_paths = deleted_files.into_iter().map(|file| file.path).collect();
    Ok(HomebrewAdoptionAuthoringReport {
        output_dir: target.display().to_string(),
        base_sha: base_sha.to_owned(),
        written_paths,
        deleted_paths,
    })
}

#[cfg(unix)]
fn publish_staged_directory(
    parent: &impl rustix::fd::AsFd,
    staged_name: &str,
    target_name: &str,
    staged_fd: &impl rustix::fd::AsFd,
    identity: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    use rustix::fs::{RenameFlags, renameat_with};
    verify_stage_identity(parent, staged_name, staged_fd, identity)?;
    renameat_with(
        parent,
        staged_name,
        parent,
        target_name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| internal("homebrew_adoption_output_publish"))?;
    verify_named_identity(parent, target_name, identity)
}

#[cfg(not(unix))]
fn publish_staged_directory(
    _parent: &impl rustix::fd::AsFd,
    _staged_name: &str,
    _target_name: &str,
    _staged_fd: &impl rustix::fd::AsFd,
    _identity: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DirectoryIdentity {
    device: i128,
    inode: u64,
    owner: u32,
    mode: i128,
}

#[cfg(unix)]
fn open_trusted_parent(path: &Path) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open, openat};
    use std::path::Component;
    if !path.is_absolute() {
        return Err(internal("homebrew_adoption_output_parent_invalid"));
    }
    let mut current = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?;
    validate_parent_directory(
        &fstat(&current).map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?,
    )?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name
            .to_str()
            .ok_or_else(|| internal("homebrew_adoption_output_parent_invalid"))?;
        current = openat(
            &current,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?;
        validate_parent_directory(
            &fstat(&current).map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?,
        )?;
    }
    if FileType::from_raw_mode(
        fstat(&current)
            .map_err(|_| internal("homebrew_adoption_output_parent_invalid"))?
            .st_mode,
    ) != FileType::Directory
    {
        return Err(internal("homebrew_adoption_output_parent_invalid"));
    }
    Ok(current)
}

#[cfg(not(unix))]
fn open_trusted_parent(_path: &Path) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

#[cfg(unix)]
fn validate_parent_directory(stat: &rustix::fs::Stat) -> Result<(), OrchestratorError> {
    use rustix::fs::FileType;
    let owner = stat.st_uid;
    let mode = stat.st_mode;
    let trusted_owner = owner == 0 || owner == crate::filesystem_owner::effective_uid();
    let writable_by_others = mode & 0o022 != 0;
    let protected_sticky_tmp = owner == 0 && mode & 0o1000 != 0;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || !trusted_owner
        || (writable_by_others && !protected_sticky_tmp)
    {
        return Err(internal("homebrew_adoption_output_parent_untrusted"));
    }
    Ok(())
}

#[cfg(unix)]
fn open_stage_directory(
    parent: &impl rustix::fd::AsFd,
    name: &str,
) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    rustix::fs::openat(
        parent,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("homebrew_adoption_output_stage"))
}

#[cfg(not(unix))]
fn open_stage_directory(
    _parent: &impl rustix::fd::AsFd,
    _name: &str,
) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

#[cfg(unix)]
fn directory_identity(fd: &impl rustix::fd::AsFd) -> Result<DirectoryIdentity, OrchestratorError> {
    let stat = rustix::fs::fstat(fd).map_err(|_| internal("homebrew_adoption_output_stage"))?;
    let identity = directory_identity_from_stat(&stat);
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
        || identity.owner != crate::filesystem_owner::effective_uid()
        || identity.mode & 0o777 != 0o700
    {
        return Err(internal("homebrew_adoption_output_stage_identity"));
    }
    Ok(identity)
}

#[cfg(unix)]
fn directory_identity_from_stat(stat: &rustix::fs::Stat) -> DirectoryIdentity {
    DirectoryIdentity {
        device: i128::from(stat.st_dev),
        inode: stat.st_ino,
        owner: stat.st_uid,
        mode: i128::from(stat.st_mode),
    }
}

#[cfg(not(unix))]
fn directory_identity(_fd: &impl rustix::fd::AsFd) -> Result<DirectoryIdentity, OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

#[cfg(unix)]
fn verify_stage_identity(
    parent: &impl rustix::fd::AsFd,
    name: &str,
    fd: &impl rustix::fd::AsFd,
    expected: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    if directory_identity(fd)? != expected {
        return Err(internal("homebrew_adoption_output_stage_identity"));
    }
    verify_named_identity(parent, name, expected)
}

#[cfg(not(unix))]
fn verify_stage_identity(
    _parent: &impl rustix::fd::AsFd,
    _name: &str,
    _fd: &impl rustix::fd::AsFd,
    _expected: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

#[cfg(unix)]
fn verify_named_identity(
    parent: &impl rustix::fd::AsFd,
    name: &str,
    expected: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    let stat = rustix::fs::statat(parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| internal("homebrew_adoption_output_stage_identity"))?;
    if directory_identity_from_stat(&stat) != expected
        || rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
    {
        return Err(internal("homebrew_adoption_output_stage_identity"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_named_identity(
    _parent: &impl rustix::fd::AsFd,
    _name: &str,
    _expected: DirectoryIdentity,
) -> Result<(), OrchestratorError> {
    Err(internal("homebrew_adoption_output_publish_unsupported"))
}

fn write_file(
    root: &Path,
    relative: &str,
    mode: u32,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    let path = safe_output_path(root, relative)?;
    if let Some(parent) = path.parent() {
        crate::exclusive_write::create_dir_no_symlink(root, parent)?;
    }
    crate::exclusive_write::write_exclusive(&path, bytes, "homebrew_adoption_output")?;
    set_mode(&path, mode)
}

fn safe_output_path(root: &Path, relative: &str) -> Result<PathBuf, OrchestratorError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || relative
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"._-/".contains(&byte)))
    {
        return Err(internal("homebrew_adoption_output_relative_path"));
    }
    Ok(root.join(relative))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
        .map_err(|_| internal("homebrew_adoption_output_permissions"))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), OrchestratorError> {
    Ok(())
}
