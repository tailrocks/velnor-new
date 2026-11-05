// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::super::rust_suites::GENERATOR_OWNED_WORKFLOW_FILES;
use super::super::workflow::WorkflowPolicy;
use super::cfg_err;
use super::rules::is_valid_workflow_slug;
use crate::errors::ContractError;

pub fn validate_preserve_workflows(
    entries: &[String],
    policy: WorkflowPolicy,
    file: &str,
) -> Result<(), ContractError> {
    if policy == WorkflowPolicy::ConsumerV1 && !entries.is_empty() {
        return Err(cfg_err(
            file,
            "workflow.preserve_workflows",
            "preserve_workflows_not_allowed_under_consumer_v1",
        ));
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        let Some(filename) = entry.strip_prefix(".github/workflows/") else {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        };
        let Some(slug) = filename.strip_suffix(".yml") else {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        };
        if !is_valid_workflow_slug(slug) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        }
        if GENERATOR_OWNED_WORKFLOW_FILES.contains(&filename) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("cannot_preserve_generator_workflow:{entry}"),
            ));
        }
        if !seen.insert(entry.as_str()) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("duplicate_preserve_workflow:{entry}"),
            ));
        }
    }
    Ok(())
}
