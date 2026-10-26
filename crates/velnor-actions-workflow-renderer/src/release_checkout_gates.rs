// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, release_jobs::ReleaseRole, release_tree::RELEASE_SOURCE_DIR};

fn expected_persist(role: ReleaseRole, sourced: bool) -> &'static str {
    let pushes = (!sourced && role == ReleaseRole::Preparation)
        || (sourced
            && matches!(
                role,
                ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
            ));
    if pushes { "true" } else { "false" }
}

pub fn check_checkout_shape(
    id: &str,
    steps: &[Step],
    role: ReleaseRole,
    sha: &str,
) -> Result<(), RenderError> {
    for step in steps {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            continue;
        };
        if !uses.starts_with("actions/checkout@") {
            continue;
        }
        let sourced = with.contains_key("path");
        if with
            .get("persist-credentials")
            .is_none_or(|value| value != expected_persist(role, sourced))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "checkout_with_credentials:{id}"
            )));
        }
        if with.get("fetch-depth").is_none_or(|value| value != "0") {
            return Err(RenderError::InvalidWorkflow(format!(
                "shallow_checkout:{id}"
            )));
        }
        if !sourced {
            if with.contains_key("ref") {
                return Err(RenderError::InvalidWorkflow(format!(
                    "pinned_policy_checkout:{id}"
                )));
            }
            continue;
        }
        if with
            .get("path")
            .is_none_or(|value| value != RELEASE_SOURCE_DIR)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "unknown_checkout_path:{id}"
            )));
        }
        if with.get("ref").is_none_or(|value| value != sha) {
            return Err(RenderError::InvalidWorkflow(format!(
                "source_ref_mismatch:{id}"
            )));
        }
    }
    Ok(())
}

fn has_exact_checkout(steps: &[Step], sha: &str) -> bool {
    steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, with, .. }
            if uses.starts_with("actions/checkout@")
                && with.get("path").is_some_and(|value| value == RELEASE_SOURCE_DIR)
                && with.get("ref").is_some_and(|value| value == sha))
    })
}

pub fn require_exact_checkout(id: &str, steps: &[Step], sha: &str) -> Result<(), RenderError> {
    if has_exact_checkout(steps, sha) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "checkout_without_exact_source:{id}"
        )))
    }
}
