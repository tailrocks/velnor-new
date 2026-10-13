//! Release IR structs: triggers, concurrency, bootstrap plan.
//!
//! Typed inputs for release rendering. Every value is validated before use;
//! the renderer branches on validated roles, never on stack or tool names.
//! Scalar validators live in the child `validators` module, re-exported here.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::ScheduleTrigger;

use velnor_actions_workflow_steps::{RenderError, steps::scan_for_private_subcommands};

mod validators;

pub(crate) use validators::is_clean_text;
pub use validators::{
    validate_environment, validate_package_name, validate_package_version, validate_plan_id,
    validate_repository, validate_source_sha,
};

/// Publisher lock and release-event eligibility (`#[path]`, no `lib.rs` edit);
/// re-exported below so `release_spec::X` paths keep working.
pub mod lock;
pub use lock::{ReleaseConcurrency, check_lock_anchor};

/// One typed `workflow_dispatch` input (plan reference, never free shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchInput {
    /// Input name (`plan`, `source_sha`, `version`, or a validated extra).
    pub name: String,
    /// Single-line description.
    pub description: String,
    /// Whether dispatch must supply the input.
    pub required: bool,
    /// Fixed default value.
    pub default: Option<String>,
}

impl DispatchInput {
    /// Rendered input type: always `string`, never boolean/choice.
    pub const INPUT_TYPE: &'static str = "string";

    /// Validate name charset plus single-line clean description/default.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] or
    /// [`RenderError::PrivateSubcommand`] for malformed inputs.
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

/// Release triggers: trusted branches, optional schedule, typed dispatch.
///
/// Structurally incapable of pull-request, `pull_request_target`, or
/// `workflow_run` events: those variants do not exist here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTriggers {
    /// Trusted branch names (exact, never globs).
    pub push_branches: Vec<String>,
    /// Optional preparation schedule.
    pub schedule: Option<ScheduleTrigger>,
    /// Typed dispatch inputs (must bind the approved plan).
    pub dispatch_inputs: Vec<DispatchInput>,
}

impl ReleaseTriggers {
    /// Validate branches, schedule, and approved-plan dispatch binding.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for bad branches, bad cron, duplicate or
    /// malformed inputs, or dispatch defaults that do not match `bootstrap`.
    pub fn validate(&self, bootstrap: &BootstrapPlan) -> Result<(), RenderError> {
        if self.push_branches.is_empty() || self.push_branches.len() > 8 {
            return Err(RenderError::InvalidWorkflow("no_push_branch".to_owned()));
        }
        for branch in &self.push_branches {
            let glob = branch.contains(['*', '?', '[', ']', '!']);
            if !is_clean_text(branch, 128) || branch.contains(char::is_whitespace) || glob {
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

    /// Require unique inputs plus required `plan`/`source_sha` binding.
    ///
    /// An approved bootstrap version additionally binds a required
    /// `version` input; without one, a `version` input is rejected as
    /// an unbound version claim.
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

/// Require one dispatch input to be required with the approved default.
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

/// Reject a dispatch input with no approved plan value behind it.
fn check_absent_input(inputs: &[DispatchInput], name: &str) -> Result<(), RenderError> {
    let present = inputs.iter().any(|input| input.name == name);
    if present {
        return Err(RenderError::InvalidWorkflow(format!(
            "dispatch_plan_mismatch:{name}"
        )));
    }
    Ok(())
}

/// Approved exact-source bootstrap plan (identities only, no credentials).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    /// Approved plan identifier referenced by dispatch inputs.
    pub plan_id: String,
    /// Exact `owner/repo` the plan is approved for.
    pub repository: String,
    /// Exact immutable source SHA the plan approves.
    pub source_sha: String,
    /// Target registry name (for example `crates-io`).
    pub registry: String,
    /// Approved package-to-version map.
    pub packages: BTreeMap<String, String>,
    /// Approved exact bootstrap version (`None` outside bootstrap mode).
    pub version: Option<String>,
}

impl BootstrapPlan {
    /// Validate every identity plus the package/version map.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] for malformed plans.
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

/// Exact publish gate: repo identity plus approved plan, source, and version.
///
/// Forks and rebound dispatches fail this condition at runtime; publish
/// jobs must carry exactly this string (see role-condition checks). The
/// version clause binds only in bootstrap mode, where the exact first
/// version is the authorization; routine publishers resolve versions at
/// runtime and carry no generation-time version.
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
