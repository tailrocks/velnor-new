// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Scenario;
use std::error::Error;

pub(super) fn dispatch(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let name = scenario.test_name();
    let source = crate::reviewed_input_helper::prepare_source()?;
    let status = source
        .command()
        .arg("cli")
        .arg(name)
        .arg(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        })
        .status()?;
    if !status.success() {
        return Err(format!("isolated GitHub fixture failed: {status}").into());
    }
    Ok(())
}

impl Scenario {
    fn test_name(self) -> &'static str {
        match self {
            #[cfg(not(debug_assertions))]
            Self::ReleaseSentinel => {
                "release_consumer_manifest_sentinel_never_bypasses_executable_binding"
            }
            Self::Valid => "absent_rest_merge_sha_reaches_owned_locator_gate",
            Self::ReversedParents => "reversed_api_merge_parents_are_rejected",
            Self::ChangedRef => "changed_merge_ref_is_rejected",
            Self::MergePendingReady => "pending_mergeability_refreshes_before_trusting_merge_ref",
            Self::MergePendingExhausted => "unresolved_mergeability_never_produces_a_plan",
            Self::MergePendingFalse => "conflicting_pull_request_finishes_mergeability_refresh",
            Self::MergePendingChanged => {
                "pull_request_changed_during_mergeability_refresh_is_rejected"
            }
            Self::MergePendingDeadline | Self::MergePendingApiDeadline => {
                "mergeability_polling_obeys_the_public_operation_deadline"
            }
            Self::HttpFailure => "github_api_http_failure_reports_safe_context",
            Self::StdoutLimit => "github_api_stdout_limit_reports_safe_context",
            Self::StderrLimit => "github_api_stderr_limit_reports_safe_context",
            Self::Deadline => "github_api_deadline_reports_safe_context",
        }
    }
}

pub(super) fn validate_worker(scenario: Option<Scenario>) -> Result<(), Box<dyn Error>> {
    super::worker::validate_worker("cli", scenario.map(Scenario::test_name))
}

pub(super) fn record_success(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    super::worker::record_success("cli", scenario.test_name())
}
