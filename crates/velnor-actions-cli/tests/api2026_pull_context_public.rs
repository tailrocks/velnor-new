// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[path = "api2026_pull_context_fixture.rs"]
mod fixture;

#[cfg(not(debug_assertions))]
#[test]
fn release_consumer_manifest_sentinel_never_bypasses_executable_binding()
-> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReleaseSentinel,
        "release_manifest_binary_mismatch",
    )
}

#[test]
fn absent_rest_merge_sha_reaches_owned_locator_gate() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::Valid,
        "homebrew_authority_locator_missing",
    )
}

#[test]
fn reversed_api_merge_parents_are_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReversedParents,
        "package_public_pull_request_merge_commit_mismatch",
    )
}

#[test]
fn changed_merge_ref_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ChangedRef,
        "package_public_pull_request_merge_ref_changed",
    )
}

#[test]
fn github_api_http_failure_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::HttpFailure, "category=nonzero")
}

#[test]
fn github_api_stdout_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StdoutLimit, "category=stdout_limit")
}

#[test]
fn github_api_stderr_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StderrLimit, "category=stderr_limit")
}

#[test]
fn github_api_deadline_reports_safe_context() -> Result<(), Box<dyn Error>> {
    if fixture::deadline_child()? {
        return Ok(());
    }
    fixture::run(fixture::Scenario::Deadline, "category=timeout")
}

#[test]
fn pending_mergeability_refreshes_before_trusting_merge_ref() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingReady,
        "homebrew_authority_locator_missing",
    )
}

#[test]
fn unresolved_mergeability_never_produces_a_plan() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingExhausted,
        "package_public_pull_request_mergeability_unresolved",
    )
}

#[test]
fn conflicting_pull_request_finishes_mergeability_refresh() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingFalse,
        "homebrew_authority_pull_request_context:runner_sha",
    )
}

#[test]
fn pull_request_changed_during_mergeability_refresh_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingChanged,
        "package_public_pull_request_changed",
    )
}

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
