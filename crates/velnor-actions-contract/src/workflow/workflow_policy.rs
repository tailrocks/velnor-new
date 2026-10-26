// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;

pub const ALLOWED_WORKFLOW_EVENTS: [&str; 5] = [
    "pull_request",
    "push",
    "merge_group",
    "schedule",
    "workflow_dispatch",
];

pub const RESTRICTED_ACTION_EVENTS: [&str; 11] = [
    "pull_request_target",
    "workflow_run",
    "issue_comment",
    "issues",
    "discussion",
    "discussion_comment",
    "fork",
    "gollum",
    "public",
    "pull_request_review",
    "pull_request_review_comment",
];

pub const DEFAULT_BLOCKED_PUBLIC_REPO_EVENTS: [&str; 1] = ["pull_request_target"];

#[must_use]
pub fn is_allowed_workflow_event(event: &str) -> bool {
    ALLOWED_WORKFLOW_EVENTS.contains(&event)
}

#[must_use]
pub fn is_restricted_action_event(event: &str) -> bool {
    RESTRICTED_ACTION_EVENTS.contains(&event)
}

pub fn validate_workflow_trigger_event(event: &str) -> Result<(), ContractError> {
    if is_restricted_action_event(event) {
        return Err(ContractError::identity(
            "trigger.event",
            format!("restricted_untrusted_actor_event:{event}"),
        ));
    }
    if !is_allowed_workflow_event(event) {
        return Err(ContractError::identity(
            "trigger.event",
            format!("unsupported_trigger_event:{event}"),
        ));
    }
    Ok(())
}

pub fn validate_workflow_trigger_events<'a>(
    events: impl IntoIterator<Item = &'a str>,
) -> Result<(), ContractError> {
    let mut seen_any = false;
    for event in events {
        seen_any = true;
        validate_workflow_trigger_event(event)?;
    }
    if !seen_any {
        return Err(ContractError::identity("trigger.event", "empty_events"));
    }
    Ok(())
}
