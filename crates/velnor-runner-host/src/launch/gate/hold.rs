// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::IntentState;
use crate::reconcile::IntentRow;

use super::GithubObservation;

const SHORT_ID: usize = 12;

/// One `legacy_hold` line per row that keeps admission closed, each naming why.
pub(super) fn hold_lines(
    rows: &[IntentRow],
    observed_docker: &[String],
    absent_docker: &[String],
    github: &GithubObservation,
) -> Vec<String> {
    rows.iter()
        .filter(|row| !row.cleanup_proven)
        .filter_map(|row| {
            let reasons = reasons(row, observed_docker, absent_docker, github);
            (!reasons.is_empty()).then(|| line(row, &reasons))
        })
        .collect()
}

fn reasons(
    row: &IntentRow,
    observed_docker: &[String],
    absent_docker: &[String],
    github: &GithubObservation,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if matches!(row.state, IntentState::Pending | IntentState::Uncertain) {
        reasons.push("launch state unsettled");
    } else if let Some(id) = row
        .docker_id
        .as_ref()
        .filter(|id| !observed_docker.contains(id))
    {
        reasons.push(if absent_docker.contains(id) {
            "container absent, cleanup not proven"
        } else {
            "container not confirmed gone"
        });
    }
    let runner = row.github_runner_id.as_deref().filter(|id| !id.is_empty());
    if let Some(runner) = runner {
        if github
            .runners
            .as_ref()
            .is_ok_and(|listed| listed.iter().any(|id| id == runner))
        {
            reasons.push("runner still listed");
        }
        reasons.extend(unobserved_reason(row, github));
    }
    reasons
}

fn unobserved_reason(row: &IntentRow, github: &GithubObservation) -> Option<&'static str> {
    let Some(scale_set) = row.scale_set_id else {
        return Some("scale set unknown");
    };
    if let Some(reason) = github.unobserved.get(&scale_set) {
        return Some(reason);
    }
    github.runners.is_err().then_some("GitHub listing failed")
}

fn line(row: &IntentRow, reasons: &[&str]) -> String {
    let state = match row.state {
        IntentState::Pending => "pending",
        IntentState::Done => "done",
        IntentState::Uncertain => "uncertain",
        IntentState::Failed => "failed",
    };
    let docker = row
        .docker_id
        .as_deref()
        .map_or("-", |id| id.get(..SHORT_ID).unwrap_or(id));
    let runner = row.github_runner_id.as_deref().unwrap_or("-");
    let why = reasons.join("; ");
    format!(
        "legacy_hold row={} state={state} docker={docker} github_runner={runner} why=\"{why}\"",
        row.id
    )
}
