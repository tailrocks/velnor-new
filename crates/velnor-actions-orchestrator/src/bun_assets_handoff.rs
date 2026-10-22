//! Credential-free export, prepare and import for Bun's private asset job.
//!
//! A single tar.gz artifact preserves executable bits and safe symlinks across
//! GitHub's artifact transport. Import uses the same bounded archive reader as
//! tool acquisition and verifies the private-fetch marker against lock pins.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::config::private_fetch_root::prepare_deps_root;
use crate::internal::internal;

mod archive;
mod manifest;

use archive::{archive_name, export_archive, only_archive};
use manifest::{Pin, parse_pins, verify_marker};

/// Operation tags carried by generated handoff steps.
pub const BUN_ASSETS_EXPORT_OP: &str = velnor_actions_contract::BUN_ASSETS_EXPORT_OPERATION;
/// Prepare the workspace and runner-temp paths before artifact download.
pub const BUN_ASSETS_PREPARE_OP: &str = velnor_actions_contract::BUN_ASSETS_PREPARE_OPERATION;
/// Import the downloaded archive and verify its lock pins.
pub const BUN_ASSETS_IMPORT_OP: &str = velnor_actions_contract::BUN_ASSETS_IMPORT_OPERATION;

/// Step environment key selecting this member's staging directory.
pub const ASSET_ID_ENV: &str = "VELNOR_BUN_ASSET_ID";
/// Step environment key carrying space-separated `id:sha256` lock pins.
pub const ASSET_PINS_ENV: &str = "VELNOR_BUN_ASSET_PINS";

const ARCHIVE_TIMEOUT: Duration = Duration::from_mins(30);

/// Run the archive producer from its fixed step environment.
///
/// # Errors
/// Rejects unsafe workspace and runner paths, invalid pins, malformed source
/// metadata, unsupported filesystem members and archive-limit violations.
pub fn run_bun_assets_export() -> Result<(), OrchestratorError> {
    let workspace = workspace_root()?;
    let (id, pins) = step_inputs()?;
    let temp = runner_velnor_root()?;
    let stage = reset_stage(&temp, &id)?;
    let velnor = real_directory(&workspace.join(".velnor"), "bun_assets_deps_unsafe")?;
    let deps = real_directory(&velnor.join("deps"), "bun_assets_deps_unsafe")?;
    verify_marker(&deps, &pins)?;
    let archive = stage.join(archive_name(&id));
    export_archive(&deps, &archive)?;
    Ok(())
}

/// Clear repository assets before the artifact download and reset its private
/// runner-temp destination.
///
/// # Errors
/// Rejects unsafe paths and filesystem failures.
pub fn run_bun_assets_prepare() -> Result<(), OrchestratorError> {
    let workspace = workspace_root()?;
    let (id, _) = step_inputs()?;
    let temp = runner_velnor_root()?;
    prepare_handoff(&workspace, &temp, &id)?;
    Ok(())
}

/// Import the downloaded tar.gz into a fresh `.velnor/deps` and check its
/// verified marker against the lock pins.
///
/// # Errors
/// Rejects unsafe archive layout, archive members, metadata or lock mismatch.
pub fn run_bun_assets_import() -> Result<(), OrchestratorError> {
    let workspace = workspace_root()?;
    let (id, pins) = step_inputs()?;
    let temp = runner_velnor_root()?;
    let stage = checked_stage(&temp, &id)?;
    let imported = import_archive(&workspace, &stage, &id, &pins);
    let cleanup = remove_path(&stage);
    imported?;
    cleanup?;
    Ok(())
}

fn prepare_handoff(workspace: &Path, temp: &Path, id: &str) -> Result<PathBuf, OrchestratorError> {
    let stage = reset_stage(temp, id)?;
    clear_deps(workspace)?;
    Ok(stage)
}

fn import_archive(
    workspace: &Path,
    stage: &Path,
    id: &str,
    pins: &[Pin],
) -> Result<PathBuf, OrchestratorError> {
    let archive = only_archive(stage, id)?;
    let deps = clear_deps(workspace)?;
    let deadline =
        CheckDeadline::after(ARCHIVE_TIMEOUT).map_err(|err| internal(&err.to_string()))?;
    let name = archive_name(id);
    if let Err(error) = extract_archive(&archive, &deps, &name, &mut ArchiveBudget::new(), deadline)
    {
        remove_path(&deps)?;
        return Err(error);
    }
    if let Err(error) = verify_marker(&deps, pins) {
        remove_path(&deps)?;
        return Err(error);
    }
    Ok(deps)
}

fn step_inputs() -> Result<(String, Vec<Pin>), OrchestratorError> {
    let id = std::env::var(ASSET_ID_ENV).map_err(|_| internal("bun_assets_id_missing"))?;
    if !safe_id(&id) {
        return Err(internal("bun_assets_id_invalid"));
    }
    let raw = std::env::var(ASSET_PINS_ENV).map_err(|_| internal("bun_assets_pins_missing"))?;
    Ok((id, parse_pins(&raw)?))
}

fn workspace_root() -> Result<PathBuf, OrchestratorError> {
    let raw = std::env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("bun_assets_workspace_missing"))?;
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(internal("bun_assets_workspace_invalid"));
    }
    fs::canonicalize(&path).map_err(|err| io_error(&path, err))
}

fn runner_velnor_root() -> Result<PathBuf, OrchestratorError> {
    let raw = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("bun_assets_runner_temp_missing"))?;
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(internal("bun_assets_runner_temp_invalid"));
    }
    let temp = fs::canonicalize(&path).map_err(|err| io_error(&path, err))?;
    real_directory(&temp.join("velnor"), "bun_assets_runner_root_unsafe")
}

fn real_directory(path: &Path, problem: &str) -> Result<PathBuf, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|err| io_error(path, err))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal(problem));
    }
    fs::canonicalize(path).map_err(|err| io_error(path, err))
}

fn reset_stage(temp: &Path, id: &str) -> Result<PathBuf, OrchestratorError> {
    let root = temp.join("bun-assets");
    ensure_real_child_dir(&root)?;
    let stage = root.join(id);
    remove_path(&stage)?;
    create_private_dir(&stage)?;
    Ok(stage)
}

fn checked_stage(temp: &Path, id: &str) -> Result<PathBuf, OrchestratorError> {
    let root = real_directory(&temp.join("bun-assets"), "bun_assets_stage_root_unsafe")?;
    real_directory(&root.join(id), "bun_assets_stage_unsafe")
}

fn ensure_real_child_dir(child: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(child) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(internal("bun_assets_stage_root_unsafe"))
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(child).map_err(|err| io_error(child, err))?;
            set_private_mode(child)?;
            Ok(())
        }
        Err(err) => Err(io_error(child, err)),
    }
}

fn create_private_dir(path: &Path) -> Result<(), OrchestratorError> {
    fs::create_dir(path).map_err(|err| io_error(path, err))?;
    set_private_mode(path)
}

fn set_private_mode(path: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| io_error(path, err))?;
    }
    Ok(())
}

fn clear_deps(workspace: &Path) -> Result<PathBuf, OrchestratorError> {
    let deps = prepare_deps_root(workspace)?;
    match fs::symlink_metadata(&deps) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(internal("bun_assets_deps_unsafe"));
        }
        Ok(_) => fs::remove_dir(&deps).map_err(|err| io_error(&deps, err))?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_error(&deps, err)),
    }
    Ok(deps)
}

fn remove_path(path: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|err| io_error(path, err))
        }
        Ok(_) => fs::remove_file(path).map_err(|err| io_error(path, err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io_error(path, err)),
    }
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn io_error(path: &Path, err: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), err.to_string())
}

#[cfg(test)]
#[path = "bun_assets_handoff_tests.rs"]
mod tests;
