// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod acquire;
mod consumer_evidence;
mod published_lock;
mod release_binary;

#[derive(Debug)]
pub enum ReleaseHelperError {
    Contract(velnor_actions_contract::ContractError),
    Problem(String),
    Io { path: String, problem: String },
    UnsafePath { path: String, reason: String },
    Render(crate::RenderError),
}

pub fn validate_consumer_manifest_for_version(
    target: velnor_actions_contract::ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<velnor_actions_contract::ReleaseManifest, ReleaseHelperError> {
    consumer_evidence::validated_consumer_manifest(target, version, json)
}

pub fn verify_binary_matches_consumer_manifest(
    manifest: &velnor_actions_contract::ReleaseManifest,
    exe_sha256: &str,
) -> Result<(), ReleaseHelperError> {
    release_binary::verify_binary_matches_consumer_manifest(manifest, exe_sha256)
}

pub fn verify_local_generator_release_manifest(
    manifest_path: &std::path::Path,
    expected_source_commit: &str,
    linux_x64_binary: &std::path::Path,
    macos_arm64_binary: &std::path::Path,
) -> Result<(), ReleaseHelperError> {
    release_binary::verify_local_generator_release_manifest(
        manifest_path,
        expected_source_commit,
        linux_x64_binary,
        macos_arm64_binary,
    )
}

pub fn validate_published_generator_lock(
    lock: &velnor_actions_contract::GeneratorLock,
) -> Result<(), ReleaseHelperError> {
    published_lock::validate_published_generator_lock(lock)
}

pub fn validate_approved_generator_lock(
    lock: &velnor_actions_contract::GeneratorLock,
    approved: &velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease<'_>,
) -> Result<(), ReleaseHelperError> {
    published_lock::validate_approved_generator_lock(lock, approved)
}

pub fn consumer_acquire_for_target(
    target: velnor_actions_contract::ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<velnor_actions_contract::Step, ReleaseHelperError> {
    acquire::consumer_acquire_for_target(target, version, json)
}

pub fn published_verdict_acquire_for_target(
    target: velnor_actions_contract::ReleaseTarget,
) -> Result<velnor_actions_contract::Step, ReleaseHelperError> {
    acquire::published_verdict_acquire_for_target(target)
}

pub fn lock_acquire_for_target(
    lock: &velnor_actions_contract::GeneratorLock,
    target: velnor_actions_contract::ReleaseTarget,
    staged: &str,
) -> Result<velnor_actions_contract::Step, ReleaseHelperError> {
    acquire::lock_acquire_for_target(lock, target, staged)
}

pub fn acquire_script_argv(
    staged: &str,
    seed_root: &str,
    target: velnor_actions_contract::ReleaseTarget,
) -> Result<Vec<String>, ReleaseHelperError> {
    acquire::acquire_script_argv(staged, seed_root, target)
}
