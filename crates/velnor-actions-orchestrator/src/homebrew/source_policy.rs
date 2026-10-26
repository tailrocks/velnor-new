// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::{
    config::HomebrewTapConfig, homebrew_authority::HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
    package_release::PackageReleaseChannel, parse_package_release_policy,
};

use crate::{
    OrchestratorError,
    internal::internal,
    prepare::{GenerationPreparation, prepare},
    safe_read::{MAX_REPO_FILE_BYTES, RepoBytes, read_repo_bytes},
};

pub fn export_homebrew_source_policy(root: &Path, output: &Path) -> Result<(), OrchestratorError> {
    if !output.is_absolute() {
        return Err(internal("homebrew_source_policy_output_must_be_absolute"));
    }
    let prep = prepare(root)?;
    require_enabled_tap(&prep)?;
    let bytes = source_policy_bytes(&prep)?;
    crate::exclusive_write::write_exclusive(output, &bytes, "homebrew_source_policy")
}

pub fn check_homebrew_source_policy(root: &Path) -> Result<(), OrchestratorError> {
    let prep = prepare(root)?;
    require_enabled_tap(&prep)?;
    check_preparation(&prep)
}

pub(crate) fn requires_native_generation_policy(tap: Option<&HomebrewTapConfig>) -> bool {
    tap.is_some_and(|tap| tap.enabled && tap.updater.is_none())
}

pub(crate) fn check_preparation(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    if !tap_enabled(prep) {
        return Ok(());
    }
    let expected = source_policy_bytes(prep)?;
    let observed = read_repo_bytes(
        &prep.root,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
        MAX_REPO_FILE_BYTES,
    )?;
    validate_policy_file(&expected, observed)
}

fn validate_policy_file(expected: &[u8], observed: RepoBytes) -> Result<(), OrchestratorError> {
    match observed {
        RepoBytes::Absent => Err(internal("homebrew_source_policy_missing")),
        RepoBytes::Bytes(bytes) => validate_policy_bytes(expected, &bytes),
    }
}

fn source_policy_bytes(prep: &GenerationPreparation) -> Result<Vec<u8>, OrchestratorError> {
    let release = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .filter(|release| release.preview.enabled)
        .ok_or_else(|| internal("homebrew_source_policy_preview_disabled"))?;
    let policy =
        crate::binary_preview_emit::release_policy(prep, release, PackageReleaseChannel::Preview)?;
    policy
        .canonical_bytes()
        .map_err(|error| internal(&error.to_string()))
}

fn require_enabled_tap(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    if !tap_enabled(prep) {
        return Err(internal("homebrew_source_policy_tap_disabled"));
    }
    Ok(())
}

fn tap_enabled(prep: &GenerationPreparation) -> bool {
    prep.config
        .distribution
        .homebrew_tap
        .as_ref()
        .is_some_and(|tap| tap.enabled)
}

fn validate_policy_bytes(expected: &[u8], observed: &[u8]) -> Result<(), OrchestratorError> {
    parse_package_release_policy(observed)
        .map_err(|_| internal("homebrew_source_policy_noncanonical"))?;
    if observed != expected {
        return Err(internal("homebrew_source_policy_stale"));
    }
    Ok(())
}
