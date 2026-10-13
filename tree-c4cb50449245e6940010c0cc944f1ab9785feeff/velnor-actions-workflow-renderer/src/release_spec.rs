//! Release IR scalars: identities, triggers, concurrency, bootstrap plan.
//!
//! Typed inputs for release rendering. Every value is validated before use;
//! the renderer branches on validated roles, never on stack or tool names.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::ScheduleTrigger;

use crate::{RenderError, steps::scan_for_private_subcommands};

/// Reject empty or overlong text plus `${{` and control characters.
pub(crate) fn is_clean_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

/// Validate a pinned environment name (charset, no expressions).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for empty, overlong, or
/// expression-carrying names.
pub fn validate_environment(name: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.');
    if is_clean_text(name, 128) && name.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_environment:{name}"
        )))
    }
}

/// True for one `owner`/`repo` segment over `[A-Za-z0-9_.-]`.
fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Validate an exact `owner/repo` identity (one slash, no expressions).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed identities.
pub fn validate_repository(repository: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_repository:{repository}"));
    if !is_clean_text(repository, 201) || repository.contains(char::is_whitespace) {
        return Err(invalid);
    }
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(invalid);
    };
    if name.contains('/') || !is_repo_segment(owner) || !is_repo_segment(name) {
        return Err(invalid);
    }
    Ok(())
}

/// Validate an exact immutable source SHA (40 lowercase hex).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for non-SHA values.
pub fn validate_source_sha(sha: &str) -> Result<(), RenderError> {
    let hex = |b: u8| b.is_ascii_digit() || matches!(b, b'a'..=b'f');
    if sha.len() == 40 && sha.bytes().all(hex) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_source_sha:{sha}"
        )))
    }
}

/// Validate an approved plan identifier (`[A-Za-z0-9_.-]`, 1..=128).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed identifiers.
pub fn validate_plan_id(id: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
    if is_clean_text(id, 128) && id.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!("bad_plan_id:{id}")))
    }
}

/// Validate a package name (leading alnum, `[A-Za-z0-9_-]`, 1..=64).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed names.
pub fn validate_package_name(name: &str) -> Result<(), RenderError> {
    let mut bytes = name.bytes();
    let leading = bytes.next().is_some_and(|b| b.is_ascii_alphanumeric());
    let rest = bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    if leading && rest && name.len() <= 64 {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_package_name:{name}"
        )))
    }
}

/// True for one dot-separated pre-release/build identifier group.
fn is_version_tail(tail: &str) -> bool {
    !tail.is_empty()
        && tail.len() <= 64
        && tail.split('.').all(|ident| {
            !ident.is_empty()
                && ident
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Validate a `X.Y.Z[-pre][+build]` package version.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed versions.
pub fn validate_package_version(version: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_package_version:{version}"));
    if !is_clean_text(version, 128) {
        return Err(invalid);
    }
    let head = match version.split_once('+') {
        Some((head, build)) if !build.contains('+') && is_version_tail(build) => head,
        Some(_) => return Err(invalid),
        None => version,
    };
    let core = match head.split_once('-') {
        Some((core, pre)) if is_version_tail(pre) => core,
        Some(_) => return Err(invalid),
        None => head,
    };
    let numeric: Vec<&str> = core.split('.').collect();
    let shaped = numeric.len() == 3
        && numeric
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if shaped { Ok(()) } else { Err(invalid) }
}

/// One typed `workflow_dispatch` input (plan reference, never free shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchInput {
    /// Input name (`plan`, `source_sha`, or a validated extra).
    pub name: String,
    /// Single-line description.
    pub description: String,
    /// Whether dispatch must supply the input.
    pub required: bool,
    /// Fixed default value.
    pub default: Option<String>,
}

impl DispatchInput {
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

/// Stable serialized lock: fixed key, publishers never cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseConcurrency {
    /// Lock group expression (stable per repository/workspace).
    pub group: String,
    /// Must always be false: never cancel an active publisher.
    pub cancel_in_progress: bool,
}

/// Tokens that would make the lock run-, version-, or source-unique.
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
    /// Validate the stable group plus the never-cancel invariant.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] or
    /// [`RenderError::PrivateSubcommand`] for unstable groups or cancel.
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

/// Require the lock key to anchor on the repository identity.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unanchored groups.
pub fn check_lock_anchor(group: &str, repository: &str) -> Result<(), RenderError> {
    if group.contains("github.repository") || group.contains(repository) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "unstable_lock_anchor".to_owned(),
        ))
    }
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
        let charset = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
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
        Ok(())
    }
}

/// Exact publish gate: repo identity plus approved plan and source binding.
///
/// Forks and rebound dispatches fail this condition at runtime; publish
/// jobs must carry exactly this string (see role-condition checks).
#[must_use]
pub fn publish_gate_condition(repository: &str, bootstrap: &BootstrapPlan) -> String {
    format!(
        "github.repository == '{repository}' && github.event.inputs.plan == '{}' && github.event.inputs.source_sha == '{}'",
        bootstrap.plan_id, bootstrap.source_sha
    )
}
