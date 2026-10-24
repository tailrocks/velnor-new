use crate::{OrchestratorError, internal::internal};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(super) fn create_fresh_directory(path: &Path) -> Result<(), OrchestratorError> {
    fs::create_dir(path).map_err(|_| internal("package_recovery_input_not_fresh"))
}

pub(super) fn create_owned_input_directory(
    root: &Path,
) -> Result<tempfile::TempDir, OrchestratorError> {
    tempfile::Builder::new()
        .prefix("release-recovery-input-")
        .tempdir_in(root)
        .map_err(|_| internal("package_recovery_input_not_fresh"))
}

pub(super) fn runner_root() -> Result<PathBuf, OrchestratorError> {
    let runner = PathBuf::from(env_text("RUNNER_TEMP")?);
    if !runner.is_absolute() {
        return Err(internal("package_recovery_runner_temp_invalid"));
    }
    let root = runner.join("velnor");
    let metadata =
        fs::symlink_metadata(&root).map_err(|_| internal("package_recovery_root_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_recovery_root_invalid"));
    }
    Ok(root)
}

pub(super) fn env_path(key: &str) -> Result<PathBuf, OrchestratorError> {
    Ok(PathBuf::from(env_text(key)?))
}

pub(super) fn env_text(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("package_recovery_context_missing"))
}

pub(super) fn positive_env(key: &str) -> Result<u64, OrchestratorError> {
    env_text(key)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_recovery_numeric_context_invalid"))
}

pub(super) fn positive_env_u32(key: &str) -> Result<u32, OrchestratorError> {
    env_text(key)?
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_recovery_numeric_context_invalid"))
}

pub(super) fn canonical_positive_env(key: &str) -> Result<u64, OrchestratorError> {
    parse_canonical_positive_decimal(&env_text(key)?)
}

pub(super) fn canonical_positive_env_u32(key: &str) -> Result<u32, OrchestratorError> {
    parse_canonical_positive_decimal_u32(&env_text(key)?)
}

fn parse_canonical_positive_decimal(value: &str) -> Result<u64, OrchestratorError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == value)
        .ok_or_else(|| internal("package_recovery_numeric_context_invalid"))
}

fn parse_canonical_positive_decimal_u32(value: &str) -> Result<u32, OrchestratorError> {
    u32::try_from(parse_canonical_positive_decimal(value)?)
        .map_err(|_| internal("package_recovery_numeric_context_invalid"))
}

pub(super) fn read_regular_bounded(path: &Path, max: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_recovery_plan_missing"))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > max {
        return Err(internal("package_recovery_plan_invalid"));
    }
    crate::release_policy_admission::metadata::read(path)
        .map_err(|_| internal("package_recovery_plan_read"))
}

#[cfg(test)]
#[path = "package_release_publish_recovery_files_tests.rs"]
mod tests;
