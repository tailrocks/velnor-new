// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[path = "api2026_pull_context_fixture.rs"]
mod fixture;

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn release_consumer_manifest_sentinel_never_bypasses_executable_binding()
-> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReleaseSentinel,
        "release_manifest_binary_mismatch",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn absent_rest_merge_sha_reaches_owned_locator_gate() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::Valid,
        "homebrew_authority_locator_missing",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn reversed_api_merge_parents_are_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReversedParents,
        "package_public_pull_request_merge_commit_mismatch",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn changed_merge_ref_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ChangedRef,
        "package_public_pull_request_merge_ref_changed",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn github_api_http_failure_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::HttpFailure, "category=nonzero")
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn github_api_stdout_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StdoutLimit, "category=stdout_limit")
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn github_api_stderr_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StderrLimit, "category=stderr_limit")
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn github_api_deadline_reports_safe_context() -> Result<(), Box<dyn Error>> {
    if fixture::deadline_child()? {
        return Ok(());
    }
    fixture::run(fixture::Scenario::Deadline, "category=timeout")
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn pending_mergeability_refreshes_before_trusting_merge_ref() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingReady,
        "homebrew_authority_locator_missing",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn unresolved_mergeability_never_produces_a_plan() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingExhausted,
        "package_public_pull_request_mergeability_unresolved",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn conflicting_pull_request_finishes_mergeability_refresh() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingFalse,
        "homebrew_authority_pull_request_context:runner_sha",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn pull_request_changed_during_mergeability_refresh_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingChanged,
        "package_public_pull_request_changed",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn mergeability_polling_obeys_the_public_operation_deadline() -> Result<(), Box<dyn Error>> {
    for scenario in [
        fixture::Scenario::MergePendingDeadline,
        fixture::Scenario::MergePendingApiDeadline,
    ] {
        fixture::run(scenario, "package_public_verification_deadline")?;
    }
    Ok(())
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn debug_consumer_generation_requires_matching_executable_manifest() -> Result<(), Box<dyn Error>> {
    fixture::generation_admission(false)
}

#[test]
fn debug_consumer_generation_requires_release_manifest() -> Result<(), Box<dyn Error>> {
    fixture::generation_admission(true)
}

#[test]
fn debug_consumer_plan_requires_release_manifest() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MissingManifest,
        "consumer_requires_release_install",
    )
}

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn consumer_plan_rejects_executable_digest_in_wrong_host_row() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::WrongHost,
        "release_manifest_binary_mismatch",
    )
}
