use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    os::fd::{AsFd, OwnedFd},
    os::unix::ffi::OsStringExt,
    path::{Component, Path},
};

use rustix::fs::{self as ufs, AtFlags, Dir, FileType, Mode, OFlags, RenameFlags};

use super::inventory::Identity;
use super::{
    OrchestratorError, ParallelTargetLane, SeedOutcome,
    execution::{SeedError, Stage},
};

pub(super) fn cleanup_stages(
    parent: &OwnedFd,
    stages: &mut Vec<Stage>,
) -> Result<(), OrchestratorError> {
    for stage in stages.drain(..) {
        let stat = match statat(parent, &stage.name) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(e)),
        };
        if !Identity::from_stat(&stat).same_object(&stage.identity)
            || FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        {
            return Err(invalid("owned_staging_directory_replaced_refusing_cleanup"));
        }
        remove_tree(&stage.fd)?;
        let current = statat(parent, &stage.name).map_err(io_error)?;
        if !Identity::from_stat(&current).same_object(&stage.identity) {
            return Err(invalid("owned_staging_directory_replaced_refusing_cleanup"));
        }
        ufs::unlinkat(parent, &stage.name, AtFlags::REMOVEDIR).map_err(io_error)?;
    }
    Ok(())
}
pub(super) fn remove_tree(parent: &OwnedFd) -> Result<(), OrchestratorError> {
    let mut names = Vec::new();
    for entry in Dir::read_from(parent).map_err(io_error)? {
        let e = entry.map_err(io_error)?;
        let name = OsString::from_vec(e.file_name().to_bytes().to_vec());
        if name.as_os_str() != OsStr::new(".") && name.as_os_str() != OsStr::new("..") {
            names.push(name);
        }
    }
    for name in names {
        let stat = statat_os(parent, &name).map_err(io_error)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                let child = openat_dir_os(parent, &name).map_err(io_error)?;
                remove_tree(&child)?;
                ufs::unlinkat(parent, &name, AtFlags::REMOVEDIR).map_err(io_error)?;
            }
            FileType::RegularFile => {
                ufs::unlinkat(parent, &name, AtFlags::empty()).map_err(io_error)?;
            }
            _ => return Err(invalid("stage_contains_special_file")),
        }
    }
    Ok(())
}

pub(super) fn open_dir_path(path: &Path) -> Result<OwnedFd, OrchestratorError> {
    ufs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io_error)
}
pub(super) fn openat_dir(parent: &impl AsFd, name: &str) -> Result<OwnedFd, std::io::Error> {
    openat_dir_os(parent, OsStr::new(name))
}
pub(super) fn openat_dir_os(parent: &impl AsFd, name: &OsStr) -> Result<OwnedFd, std::io::Error> {
    ufs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(Into::into)
}
pub(super) fn open_dir_parts(
    root: &OwnedFd,
    parts: &[OsString],
) -> Result<OwnedFd, std::io::Error> {
    let mut current = root.try_clone()?;
    for part in parts {
        current = openat_dir_os(&current, part)?;
    }
    Ok(current)
}
pub(super) fn openat_parts(root: &OwnedFd, parts: &[OsString]) -> Result<OwnedFd, SeedError> {
    let mut current = root.try_clone().map_err(io_error)?;
    for part in parts {
        current = openat_dir_os(&current, part).map_err(io_error)?;
    }
    Ok(current)
}
pub(super) fn open_parent(
    root: &OwnedFd,
    parts: &[OsString],
) -> Result<(OwnedFd, OsString), SeedError> {
    let name = parts
        .last()
        .ok_or_else(|| fail("source_path_empty"))?
        .clone();
    Ok((openat_parts(root, &parts[..parts.len() - 1])?, name))
}
pub(super) fn statat(parent: &impl AsFd, name: &str) -> Result<rustix::fs::Stat, std::io::Error> {
    statat_os(parent, OsStr::new(name))
}
pub(super) fn statat_os(
    parent: &impl AsFd,
    name: &OsStr,
) -> Result<rustix::fs::Stat, std::io::Error> {
    ufs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(Into::into)
}
pub(super) fn rename_noreplace(
    parent: &impl AsFd,
    source: &str,
    destination: &str,
) -> Result<(), std::io::Error> {
    ufs::renameat_with(parent, source, parent, destination, RenameFlags::NOREPLACE)
        .map_err(Into::into)
}
pub(super) fn random_stage_name() -> Result<String, OrchestratorError> {
    let mut random = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(io_error)?;
    let mut out = String::from(".velnor-seed-");
    for b in random {
        out.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(b & 0x0f)]));
    }
    Ok(out)
}
pub(super) fn validate_target(value: &str) -> Result<Vec<OsString>, OrchestratorError> {
    let path = Path::new(value);
    if value.starts_with('/')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid("invalid_scoped_target"));
    }
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.last().copied() != Some("target")
        || parts.iter().any(|part| !safe_part(part))
    {
        return Err(invalid("invalid_scoped_target"));
    }
    Ok(parts.into_iter().map(OsString::from).collect())
}
pub(super) fn validate_lanes(lanes: &[ParallelTargetLane]) -> Result<(), OrchestratorError> {
    if lanes.is_empty() || !lanes.windows(2).all(|w| w[0] < w[1]) {
        return Err(invalid("invalid_target_lanes"));
    }
    Ok(())
}
pub(super) fn parse_lanes(text: &str) -> Result<Vec<ParallelTargetLane>, OrchestratorError> {
    let lanes = text
        .split(',')
        .map(|v| ParallelTargetLane::parse(v).ok_or_else(|| invalid("invalid_target_lanes")))
        .collect::<Result<Vec<_>, _>>()?;
    validate_lanes(&lanes)?;
    if lanes
        .iter()
        .map(|l| l.as_str())
        .collect::<Vec<_>>()
        .join(",")
        != text
    {
        return Err(invalid("noncanonical_target_lanes"));
    }
    Ok(lanes)
}
pub(super) fn safe_part(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}
pub(super) fn skip(reason: &'static str) -> SeedOutcome {
    println!("Parallel targets: optional seed skipped: {reason}");
    SeedOutcome::Skipped { reason }
}
pub(super) fn invalid(value: &'static str) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: format!("parallel_target_seed: {value}"),
    }
}
pub(super) fn fail(value: &'static str) -> SeedError {
    SeedError::Contract(invalid(value))
}
pub(super) fn io_error(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io("parallel_target_seed", error.to_string())
}
pub(super) fn seed_io_error(error: impl Into<std::io::Error>) -> SeedError {
    SeedError::Io(error.into())
}
pub(super) fn capacity_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(28 | 122))
}
pub(super) fn unsupported_rename(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(45 | 95 | 38))
}
