// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::is_valid_branch_name;

use crate::{RenderError, steps::scan_for_private_subcommands};

#[path = "release_validators.rs"]
mod validators;

pub(crate) use validators::{is_clean_text, validate_version_group};
pub use validators::{
    validate_environment, validate_package_name, validate_package_version, validate_plan_id,
    validate_repository, validate_source_sha,
};

#[path = "release_lock.rs"]
pub mod lock;
pub use lock::{ReleaseConcurrency, check_lock_anchor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchInput {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

impl DispatchInput {
    pub const INPUT_TYPE: &'static str = "string";

    pub fn validate(&self) -> Result<(), RenderError> {
        let charset =
            |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-');
        if self.name.is_empty() || self.name.len() > 64 || !self.name.bytes().all(charset) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_dispatch_name:{}",
                self.name
            )));
        }
        if !is_clean_text(&self.description, 256) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_dispatch_description:{}",
                self.name
            )));
        }
        if let Some(default) = &self.default
            && !is_clean_text(default, 256)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_dispatch_default:{}",
                self.name
            )));
        }
        scan_for_private_subcommands(&self.name)?;
        scan_for_private_subcommands(&self.description)?;
        if let Some(default) = &self.default {
            scan_for_private_subcommands(default)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTriggers {
    pub push_branches: Vec<String>,
    pub dispatch_inputs: Vec<DispatchInput>,
}

impl ReleaseTriggers {
    pub fn validate(&self, plan: &ReleasePlan) -> Result<(), RenderError> {
        if (plan.dispatch && !self.push_branches.is_empty())
            || (!plan.dispatch && self.push_branches.len() != 1)
        {
            return Err(RenderError::InvalidWorkflow(
                "release_requires_one_default_branch".to_owned(),
            ));
        }
        for branch in &self.push_branches {
            if !is_valid_branch_name(branch) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "bad_push_branch:{branch}"
                )));
            }
            scan_for_private_subcommands(branch)?;
        }
        self.check_dispatch_binding(plan)
    }

    fn check_dispatch_binding(&self, plan: &ReleasePlan) -> Result<(), RenderError> {
        let mut names = BTreeSet::new();
        for input in &self.dispatch_inputs {
            input.validate()?;
            if !names.insert(input.name.clone()) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "duplicate_dispatch_input:{}",
                    input.name
                )));
            }
        }
        if plan.dispatch {
            let expected_len = if plan.version.is_some() { 3 } else { 2 };
            if self.dispatch_inputs.len() != expected_len {
                return Err(RenderError::InvalidWorkflow(
                    "unexpected_release_dispatch_input".to_owned(),
                ));
            }
            check_bound_input(&self.dispatch_inputs, "plan", &plan.plan_id)?;
            check_bound_input(&self.dispatch_inputs, "source_sha", &plan.source_sha)?;
            if let Some(version) = &plan.version {
                check_bound_input(&self.dispatch_inputs, "version", version)?;
            }
        } else if !self.dispatch_inputs.is_empty() || plan.version.is_some() {
            return Err(RenderError::InvalidWorkflow(
                "unexpected_release_dispatch".to_owned(),
            ));
        }
        Ok(())
    }
}

fn check_bound_input(
    inputs: &[DispatchInput],
    name: &str,
    approved: &str,
) -> Result<(), RenderError> {
    let bound = inputs.iter().any(|input| {
        input.name == name && input.required && input.default.as_deref() == Some(approved)
    });
    if bound {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "dispatch_plan_mismatch:{name}"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub plan_id: String,
    pub repository: String,
    pub source_sha: String,
    pub registry: String,
    pub packages: BTreeMap<String, String>,
    pub version: Option<String>,
    pub dispatch: bool,
}

impl ReleasePlan {
    pub fn validate(&self) -> Result<(), RenderError> {
        validate_plan_id(&self.plan_id)?;
        validate_repository(&self.repository)?;
        validate_source_sha(&self.source_sha)?;
        let charset =
            |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_');
        if !is_clean_text(&self.registry, 64) || !self.registry.bytes().all(charset) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_registry:{}",
                self.registry
            )));
        }
        if self.packages.is_empty() || self.packages.len() > 64 {
            return Err(RenderError::InvalidWorkflow(
                "no_release_packages".to_owned(),
            ));
        }
        for (name, version) in &self.packages {
            validate_package_name(name)?;
            validate_package_version(version)?;
        }
        if let Some(version) = &self.version {
            validate_package_version(version)?;
        }
        Ok(())
    }
}

#[must_use]
pub fn publish_gate_condition(repository: &str, branch: &str, plan: &ReleasePlan) -> String {
    let event = format!("github.event_name == 'push' && github.ref == 'refs/heads/{branch}'");
    let event = if plan.dispatch {
        let mut dispatch = format!(
            "github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/{branch}' && github.event.inputs.plan == '{}' && github.event.inputs.source_sha == '{}'",
            plan.plan_id, plan.source_sha
        );
        if let Some(version) = &plan.version {
            dispatch.push_str(" && github.event.inputs.version == '");
            dispatch.push_str(version);
            dispatch.push('\'');
        }
        dispatch
    } else {
        event
    };
    format!(
        "github.repository == '{repository}' && ({event}) && needs.release-preflight.outputs.eligible == 'true'"
    )
}
