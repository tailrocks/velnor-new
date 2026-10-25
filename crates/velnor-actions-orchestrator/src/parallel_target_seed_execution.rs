use std::{ffi::OsString, fs, os::fd::OwnedFd, path::Path, time::Instant};

use rustix::fs::{self as ufs, FileType, Mode};

use super::filesystem::{invalid, io_error, skip};
use super::{
    OrchestratorError, ParallelTargetLane, SeedLimits, SeedOutcome,
    filesystem::{
        capacity_error, cleanup_stages, fail, open_dir_parts, open_dir_path, openat_dir,
        random_stage_name, rename_noreplace, seed_io_error, statat, unsupported_rename,
        validate_lanes, validate_target,
    },
    inventory::{Entry, Identity, Snapshot},
};

pub(super) fn seed_parallel_targets_with_limits(
    workspace: &Path,
    target_root: &str,
    lanes: &[ParallelTargetLane],
    limits: SeedLimits,
) -> Result<SeedOutcome, OrchestratorError> {
    let Some(target) = prepare_target(workspace, target_root, lanes)? else {
        return Ok(SeedOutcome::Cold);
    };
    let snapshot = match capture_snapshot(&target, limits) {
        Ok(snapshot) => snapshot,
        Err(error) => return finish_seed_error(error),
    };
    stage_and_admit(&target, &snapshot)
}

struct SeedTarget {
    fd: OwnedFd,
    path: std::path::PathBuf,
    identity: Identity,
    cold: Vec<ParallelTargetLane>,
    debug_stat: rustix::fs::Stat,
}

fn prepare_target(
    workspace: &Path,
    target_root: &str,
    lanes: &[ParallelTargetLane],
) -> Result<Option<SeedTarget>, OrchestratorError> {
    let parts = validate_target(target_root)?;
    validate_lanes(lanes)?;
    if !workspace.is_absolute() {
        return Err(invalid("workspace_not_absolute"));
    }

    let workspace_fd = open_dir_path(workspace)?;
    let target_fd = match open_dir_parts(&workspace_fd, &parts) {
        Ok(fd) => fd,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("Parallel targets: no baseline; checks run cold");
            return Ok(None);
        }
        Err(error) => return Err(io_error(error)),
    };
    let target_path = workspace.join(target_root);
    let target_meta = fs::symlink_metadata(&target_path).map_err(io_error)?;
    if !target_meta.is_dir() || target_meta.file_type().is_symlink() {
        return Err(invalid("target_not_real_directory"));
    }
    let target_identity = Identity::from_metadata(&target_meta);
    if Identity::from_stat(&ufs::fstat(&target_fd).map_err(io_error)?) != target_identity {
        return Err(invalid("target_path_changed"));
    }

    let mut cold = Vec::new();
    for lane in lanes {
        match statat(&target_fd, lane.as_str()) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => {
                println!("Parallel targets: preserve existing {}", lane.as_str());
            }
            Ok(_) => return Err(invalid("existing_lane_not_directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => cold.push(*lane),
            Err(error) => return Err(io_error(error)),
        }
    }
    if cold.is_empty() {
        return Ok(None);
    }

    let debug_stat = match statat(&target_fd, "debug") {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => stat,
        Ok(_) => return Err(invalid("debug_not_directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("Parallel targets: no debug baseline; checks run cold");
            return Ok(None);
        }
        Err(error) => return Err(io_error(error)),
    };

    Ok(Some(SeedTarget {
        fd: target_fd,
        path: target_path,
        identity: target_identity,
        cold,
        debug_stat,
    }))
}

fn capture_snapshot(target: &SeedTarget, limits: SeedLimits) -> Result<Snapshot, SeedError> {
    let mut snapshot = Snapshot::new(Instant::now(), limits);
    let debug_fd =
        openat_dir(&target.fd, "debug").map_err(|error| SeedError::Contract(io_error(error)))?;
    if !Identity::from_stat(&ufs::fstat(&debug_fd).map_err(seed_io_error)?)
        .matches_stat(&target.debug_stat)
    {
        return Err(fail("source_directory_changed"));
    }
    snapshot
        .entries
        .push(Entry::new(vec![OsString::from("debug")], target.debug_stat));
    snapshot.inventory(&debug_fd, &mut Vec::from([OsString::from("debug")]))?;
    match statat(&target.fd, ".rustc_info.json") {
        Ok(compiler) => {
            if FileType::from_raw_mode(compiler.st_mode) != FileType::RegularFile {
                return Err(fail("compiler_metadata_not_regular"));
            }
            snapshot.add_bytes(u64::try_from(compiler.st_size.max(0)).unwrap_or_default())?;
            snapshot.entries.push(Entry::new(
                vec![OsString::from(".rustc_info.json")],
                compiler,
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(SeedError::Contract(io_error(error))),
    }
    verify_copy_capacity(target, &snapshot, limits)?;
    Ok(snapshot)
}

fn verify_copy_capacity(
    target: &SeedTarget,
    snapshot: &Snapshot,
    limits: SeedLimits,
) -> Result<(), SeedError> {
    let total = snapshot
        .bytes
        .checked_mul(target.cold.len() as u64)
        .ok_or_else(|| fail("byte_budget_overflow"))?;
    if total > limits.bytes {
        return Err(SeedError::Skip("byte limit"));
    }
    let available = ufs::statvfs(&target.path)
        .map(|stats| stats.f_bavail.saturating_mul(stats.f_frsize))
        .map_err(|error| SeedError::Contract(io_error(error)))?;
    if available < total.saturating_add(limits.reserve) {
        return Err(SeedError::Skip("byte or free-disk limit"));
    }
    Ok(())
}

fn stage_and_admit(
    target: &SeedTarget,
    snapshot: &Snapshot,
) -> Result<SeedOutcome, OrchestratorError> {
    let mut stages = Vec::new();
    let result = populate_and_admit_stages(target, snapshot, &mut stages);
    finalize_staged_result(
        &target.fd,
        &mut stages,
        result,
        &target.cold,
        snapshot.bytes,
    )
}

fn populate_and_admit_stages(
    target: &SeedTarget,
    snapshot: &Snapshot,
    stages: &mut Vec<Stage>,
) -> Result<(), SeedError> {
    for _lane in &target.cold {
        snapshot.checkpoint()?;
        let stage_name = random_stage_name()?;
        ufs::mkdirat(&target.fd, stage_name.as_str(), Mode::from_raw_mode(0o700))
            .map_err(seed_io_error)?;
        let stage_fd = openat_dir(&target.fd, &stage_name).map_err(seed_io_error)?;
        let identity = Identity::from_stat(&ufs::fstat(&stage_fd).map_err(seed_io_error)?);
        stages.push(Stage {
            name: stage_name,
            fd: stage_fd,
            identity,
        });
        let Some(stage) = stages.last() else {
            return Err(fail("staging_directory_missing"));
        };
        ensure_stage_identity(&target.fd, stage)?;
        snapshot.copy_to(&target.fd, &stage.fd)?;
    }
    snapshot.validate_source(&target.fd)?;
    admit_stages(target, snapshot, stages)
}

fn admit_stages(
    target: &SeedTarget,
    snapshot: &Snapshot,
    stages: &[Stage],
) -> Result<(), SeedError> {
    admit_stages_with_rename(target, snapshot, stages, rename_stage)
}

fn ensure_stage_identity(parent: &OwnedFd, stage: &Stage) -> Result<(), SeedError> {
    if !statat(parent, &stage.name).is_ok_and(|stat| stage.identity.matches_stat(&stat)) {
        return Err(fail("staging_directory_replaced"));
    }
    Ok(())
}

fn admit_stages_with_rename(
    target: &SeedTarget,
    snapshot: &Snapshot,
    stages: &[Stage],
    mut rename: impl FnMut(&SeedTarget, ParallelTargetLane, &Stage) -> Result<(), SeedError>,
) -> Result<(), SeedError> {
    for (lane, stage) in target.cold.iter().zip(stages) {
        snapshot.checkpoint()?;
        let current = fs::symlink_metadata(&target.path).map_err(seed_io_error)?;
        // Creating staging directories changes the target directory's timestamps.
        // Its inode identity is the relevant replacement guard here.
        if !Identity::from_metadata(&current).same_object(&target.identity) {
            return Err(fail("target_directory_replaced"));
        }
        if !Identity::from_stat(&statat(&target.fd, &stage.name).map_err(seed_io_error)?)
            .same_object(&stage.identity)
        {
            return Err(fail("staging_directory_replaced"));
        }
        rename(target, *lane, stage)?;
        if !Identity::from_stat(&statat(&target.fd, lane.as_str()).map_err(seed_io_error)?)
            .same_object(&stage.identity)
        {
            return Err(fail("admitted_lane_identity_changed"));
        }
        println!(
            "Parallel targets: seeded {}: {} bytes",
            lane.as_str(),
            snapshot.bytes
        );
    }
    Ok(())
}

fn rename_stage(
    target: &SeedTarget,
    lane: ParallelTargetLane,
    stage: &Stage,
) -> Result<(), SeedError> {
    match rename_noreplace(&target.fd, &stage.name, lane.as_str()) {
        Ok(()) => Ok(()),
        Err(error) if unsupported_rename(&error) => {
            Err(SeedError::Skip("atomic exclusive rename unsupported"))
        }
        Err(error) if capacity_error(&error) => Err(SeedError::Skip("storage capacity exhausted")),
        Err(error) => Err(SeedError::Io(error)),
    }
}

// Further implementation details follow in helper types/functions.

#[derive(Debug)]
pub(super) enum SeedError {
    Skip(&'static str),
    Contract(OrchestratorError),
    Io(std::io::Error),
}
impl From<OrchestratorError> for SeedError {
    fn from(value: OrchestratorError) -> Self {
        Self::Contract(value)
    }
}

fn finish_seed_error(error: SeedError) -> Result<SeedOutcome, OrchestratorError> {
    match error {
        SeedError::Skip(reason) => Ok(skip(reason)),
        SeedError::Contract(error) => Err(error),
        SeedError::Io(error) if capacity_error(&error) => Ok(skip("storage capacity exhausted")),
        SeedError::Io(error) => Err(io_error(error)),
    }
}

fn classify_seed_result(
    result: Result<(), SeedError>,
    lanes: &[ParallelTargetLane],
    bytes: u64,
) -> Result<SeedOutcome, OrchestratorError> {
    match result {
        Ok(()) => Ok(SeedOutcome::Seeded {
            lanes: lanes.iter().map(|lane| lane.as_str().to_owned()).collect(),
            bytes_per_lane: bytes,
        }),
        Err(SeedError::Skip(reason)) => Ok(skip(reason)),
        Err(SeedError::Contract(error)) => Err(error),
        Err(SeedError::Io(error)) if capacity_error(&error) => {
            Ok(skip("storage capacity exhausted"))
        }
        Err(SeedError::Io(error)) => Err(io_error(error)),
    }
}

pub(super) fn finalize_staged_result(
    parent: &OwnedFd,
    stages: &mut Vec<Stage>,
    result: Result<(), SeedError>,
    lanes: &[ParallelTargetLane],
    bytes: u64,
) -> Result<SeedOutcome, OrchestratorError> {
    cleanup_stages(parent, stages)?;
    classify_seed_result(result, lanes, bytes)
}

pub(super) struct Stage {
    pub(super) name: String,
    pub(super) fd: OwnedFd,
    pub(super) identity: Identity,
}

#[cfg(test)]
#[path = "parallel_target_seed_execution_tests.rs"]
mod tests;
