// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{ScheduleTrigger, is_valid_branch_name};

use crate::{RenderError, steps::scan_for_private_subcommands};

#[path = "release_validators.rs"]
mod validators;

pub(crate) use validators::is_clean_text;
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
    pub schedule: Option<ScheduleTrigger>,
    pub dispatch_inputs: Vec<DispatchInput>,
}

impl ReleaseTriggers {
    pub fn validate(&self, bootstrap: &BootstrapPlan) -> Result<(), RenderError> {
        if self.push_branches.is_empty() || self.push_branches.len() > 8 {
            return Err(RenderError::InvalidWorkflow("no_push_branch".to_owned()));
        }
        for branch in &self.push_branches {
            if !is_valid_branch_name(branch) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "bad_push_branch:{branch}"
                )));
            }
            scan_for_private_subcommands(branch)?;
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate().map_err(RenderError::Contract)?;
        }
        self.check_dispatch_binding(bootstrap)
    }

    fn check_dispatch_binding(&self, bootstrap: &BootstrapPlan) -> Result<(), RenderError> {
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
        check_bound_input(&self.dispatch_inputs, "plan", &bootstrap.plan_id)?;
        check_bound_input(&self.dispatch_inputs, "source_sha", &bootstrap.source_sha)?;
        match &bootstrap.version {
            Some(version) => check_bound_input(&self.dispatch_inputs, "version", version)?,
            None => check_absent_input(&self.dispatch_inputs, "version")?,
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

fn check_absent_input(inputs: &[DispatchInput], name: &str) -> Result<(), RenderError> {
    let present = inputs.iter().any(|input| input.name == name);
    if present {
        return Err(RenderError::InvalidWorkflow(format!(
            "dispatch_plan_mismatch:{name}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub plan_id: String,
    pub repository: String,
    pub source_sha: String,
    pub registry: String,
    pub packages: BTreeMap<String, String>,
    pub version: Option<String>,
}

impl BootstrapPlan {
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
pub fn publish_gate_condition(repository: &str, bootstrap: &BootstrapPlan) -> String {
    let base = format!(
        "github.repository == '{repository}' && github.event.inputs.plan == '{}' && github.event.inputs.source_sha == '{}'",
        bootstrap.plan_id, bootstrap.source_sha
    );
    match &bootstrap.version {
        Some(version) => format!("{base} && github.event.inputs.version == '{version}'"),
        None => base,
    }
}
