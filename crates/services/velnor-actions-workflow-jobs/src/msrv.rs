//! Per-crate MSRV qualification (rust-quality §§2/9, workflow §3).
//!
//! MSRV checks run in the pinned toolchain-qualification workflow on
//! updates/releases only — never in pull-request workflows. This module
//! builds per-crate MSRV steps/jobs over orchestrator-supplied argv (the
//! Mise tool must equal workspace `rust-version`, commands use `--locked`)
//! and rejects any MSRV content from PR renders. Qualification-workflow
//! triggers, file emission, and scheduling stay orchestrator-owned.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepRole};

use velnor_actions_workflow_steps::{RenderError, steps};

/// Typed per-crate MSRV check: product crate plus declared minimum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsrvSpec {
    /// Product crate name (Cargo charset).
    pub package: String,
    /// Workspace `rust-version` the Mise tool must equal (`major.minor`).
    pub rust_version: String,
}

impl MsrvSpec {
    /// Validate the crate name and `major.minor` MSRV spelling.
    /// # Errors
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.package.is_empty()
            || !self
                .package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(RenderError::BadCommand(format!(
                "msrv_bad_package:{}",
                self.package
            )));
        }
        let mut parts = self.rust_version.split('.');
        let shaped = matches!((parts.next(), parts.next(), parts.next()), (Some(major), Some(minor), None)
            if !major.is_empty()
                && !minor.is_empty()
                && major.bytes().all(|b| b.is_ascii_digit())
                && minor.bytes().all(|b| b.is_ascii_digit()));
        if shaped {
            Ok(())
        } else {
            Err(RenderError::BadCommand(format!(
                "msrv_bad_rust_version:{}",
                self.rust_version
            )))
        }
    }
}

/// MSRV step display name for one product crate.
#[must_use]
pub fn msrv_step_name(package: &str) -> String {
    format!("msrv {package}")
}

/// Per-crate MSRV step: Mise tool equals `rust-version`, `--locked` set.
///
/// The argv arrives from the orchestrator's Mise vectors; the renderer
/// verifies the RQ-9.6 shape (exact `rust@<rust-version>` tool element
/// plus `--locked`) instead of trusting the caller.
/// # Errors
pub fn msrv_step(
    spec: &MsrvSpec,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    spec.validate()?;
    let tool = format!("rust@{}", spec.rust_version);
    if !argv.iter().any(|arg| arg == &tool) {
        return Err(RenderError::BadCommand(
            "msrv_tool_not_rust_version".to_owned(),
        ));
    }
    if !argv.iter().any(|arg| arg == "--locked") {
        return Err(RenderError::BadCommand("msrv_without_locked".to_owned()));
    }
    let mut step = steps::shell_step(&msrv_step_name(&spec.package), argv, env)?;
    step.role = Some(StepRole::MsrvQualification);
    Ok(step)
}

/// Per-crate MSRV job: checkout plus the MSRV step, no matrix fan-in.
///
/// Jobs are independent (one per product crate); the qualification
/// workflow's triggers and setup/tooling prelude are orchestrator-owned.
/// # Errors
pub fn msrv_job(
    label: &str,
    checkout_uses: &str,
    spec: &MsrvSpec,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Job, RenderError> {
    spec.validate()?;
    Ok(Job {
        outputs: Vec::new(),
        display_name: format!("MSRV {}", spec.package),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::MSRV,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            steps::checkout_step(checkout_uses)?,
            msrv_step(spec, argv, env)?,
        ],
    })
}

/// Reject any MSRV job or step from a pull-request workflow render.
///
/// MSRV verification belongs to the pinned toolchain-qualification
/// workflow only; its presence here fails closed instead of repeating
/// per-crate minimum-version checks on every pull request.
/// # Errors
pub fn check_no_msrv(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let leaking = id.to_lowercase().contains("msrv")
            || job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::MsrvQualification));
        if leaking {
            return Err(RenderError::InvalidWorkflow(format!(
                "msrv_in_pr_workflow:{id}"
            )));
        }
    }
    Ok(())
}
