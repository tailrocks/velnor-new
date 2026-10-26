// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{is_clean_text, validate_repository};
use crate::{RenderError, steps::scan_for_private_subcommands};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseConcurrency {
    pub group: String,
    pub cancel_in_progress: bool,
}

const FORBIDDEN_LOCK_TOKENS: &[&str] = &[
    "run_id",
    "run_attempt",
    "run_number",
    "github.sha",
    "github.ref",
    "github.event",
    "inputs.",
    "matrix.",
    "version",
    "strategy",
];

impl ReleaseConcurrency {
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.group, 256) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_lock_group:{}",
                self.group
            )));
        }
        for token in FORBIDDEN_LOCK_TOKENS {
            if self.group.contains(token) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "forbidden_lock_token:{token}"
                )));
            }
        }
        scan_for_private_subcommands(&self.group)?;
        if self.cancel_in_progress {
            return Err(RenderError::InvalidWorkflow("publisher_cancel".to_owned()));
        }
        Ok(())
    }
}

pub fn check_lock_anchor(group: &str, repository: &str) -> Result<(), RenderError> {
    if group.contains("github.repository") || group.contains(repository) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "unstable_lock_anchor".to_owned(),
        ))
    }
}

pub fn stable_lock_group(
    registry: &str,
    repository: &str,
    workspace: &str,
) -> Result<ReleaseConcurrency, RenderError> {
    validate_lock_segment(registry, "registry")?;
    validate_repository(repository)?;
    validate_lock_segment(workspace, "workspace")?;
    Ok(ReleaseConcurrency {
        group: format!("release-{registry}-{repository}-{workspace}"),
        cancel_in_progress: false,
    })
}

fn validate_lock_segment(segment: &str, kind: &str) -> Result<(), RenderError> {
    let shaped = !segment.is_empty()
        && segment.len() <= 64
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
    if shaped {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_lock_{kind}:{segment}"
        )))
    }
}

const RELEASE_ELIGIBLE_EVENTS: &[&str] = &["push", "schedule", "workflow_dispatch"];

pub fn reject_prohibited_release_event(event: &str) -> Result<(), RenderError> {
    if RELEASE_ELIGIBLE_EVENTS.contains(&event) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "prohibited_release_event:{event}"
        )))
    }
}
