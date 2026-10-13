//! Pinned Mise setup: template plus strict insertion gates.
//!
//! Every job that invokes `mise` must be preceded by the pinned
//! `jdx/mise-action` step (workflow-contract §3 steps 1-3, task prelude).
//! Pins arrive as typed [`MiseSetup`] from the orchestrator's compiled
//! catalog; the renderer never invents them and fails closed on malformed
//! or misordered setup steps instead of emitting `mise`-less YAML.

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{RenderError, steps};

/// Pinned Mise setup action name.
pub const MISE_ACTION_NAME: &str = "jdx/mise-action";
/// Contract-fixed display name of the setup step.
pub const SETUP_MISE_NAME: &str = "Setup Mise";

/// Typed Mise setup pins: action ref plus exact binary identity.
///
/// The orchestrator resolves `[actions.overrides]` and the compiled
/// catalog before constructing this; `sha256` is the digest of the
/// extracted `mise` binary for the single runner platform (the action
/// compares the input against the installed binary, not the archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetup {
    /// Full-SHA `jdx/mise-action` ref.
    pub uses: String,
    /// Exact Mise release (`MISE_VERSION` catalog format, never `latest`).
    pub version: String,
    /// Lowercase hex SHA-256 of the installed `mise` binary.
    pub sha256: String,
}

impl MiseSetup {
    /// Validate every pin before any step is built from it.
    /// # Errors
    pub fn validate(&self) -> Result<(), RenderError> {
        steps::validate_uses(&self.uses)?;
        if !self.uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            return Err(RenderError::BadActionRef(format!(
                "not_mise_action:{}",
                self.uses
            )));
        }
        if !is_catalog_version(&self.version) {
            return Err(RenderError::BadCommand(format!(
                "bad_mise_version:{}",
                self.version
            )));
        }
        if self.sha256.len() != 64 || !is_lower_hex(&self.sha256) {
            return Err(RenderError::BadCommand("bad_mise_sha256".to_owned()));
        }
        Ok(())
    }
}

/// Fixed `Setup Mise` step: exact `version`/`sha256`, no install or env.
///
/// `install: false` keeps project tool files, tasks, and hooks from
/// running; `env: false` keeps Mise env out of subsequent steps. The
/// `with` map is exactly these four keys.
/// # Errors
pub fn mise_setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup.validate()?;
    steps::action_step(
        SETUP_MISE_NAME,
        &setup.uses,
        std::collections::BTreeMap::from([
            ("version".to_owned(), setup.version.clone()),
            ("sha256".to_owned(), setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
        ]),
    )
}

/// True when any shell step invokes the `mise` program.
pub(crate) fn job_uses_mise(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mise"))
    })
}

/// Ensure a well-formed setup step precedes every `mise` use.
///
/// `always` covers jobs whose `mise` use is dynamic (matrix `run`
/// payloads) or contract-mandated (plan/task preludes): they get the
/// step even with no static `mise` argv. Present steps must be exactly
/// one, well-formed, and before the first `mise` use; anything else
/// fails closed instead of emitting a `mise: command not found` job.
/// # Errors
pub(crate) fn ensure_setup(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
) -> Result<(), RenderError> {
    setup.validate()?;
    let present: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect();
    if present.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    if let Some(&index) = present.first() {
        check_setup_shape(job_id, &job.steps[index])?;
        if let Some(first_mise) = first_mise_index(job)
            && index > first_mise
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "setup_mise_misordered:{job_id}"
            )));
        }
        return Ok(());
    }
    if always || job_uses_mise(job) {
        let at = insert_at(job).min(job.steps.len());
        job.steps.insert(at, setup_step(setup)?);
    }
    Ok(())
}

/// Build the validated setup step (shape checked once more on insert).
fn setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    let step = mise_setup_step(setup)?;
    debug_assert!(is_setup_step(&step));
    Ok(step)
}

/// Insert after a leading Checkout step, else at the front.
fn insert_at(job: &Job) -> usize {
    job.steps
        .first()
        .filter(|step| step.name == "Checkout")
        .map_or(0, |_| 1)
}

/// Index of the first shell step invoking `mise`, when any.
fn first_mise_index(job: &Job) -> Option<usize> {
    job.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mise"))
    })
}

/// True for `jdx/mise-action` steps regardless of shape.
fn is_setup_step(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with(&format!("{MISE_ACTION_NAME}@")))
}

/// Reject malformed setup steps: exact inputs, nothing else.
fn check_setup_shape(job_id: &str, step: &Step) -> Result<(), RenderError> {
    let StepKind::Action { uses, with } = &step.kind else {
        return Err(setup_malformed(job_id));
    };
    if steps::validate_uses(uses).is_err() {
        return Err(setup_malformed(job_id));
    }
    let shape_ok = with.len() == 4
        && with.get("install").is_some_and(|v| v == "false")
        && with.get("env").is_some_and(|v| v == "false")
        && with.get("version").is_some_and(|v| is_catalog_version(v))
        && with
            .get("sha256")
            .is_some_and(|v| v.len() == 64 && is_lower_hex(v));
    if shape_ok {
        Ok(())
    } else {
        Err(setup_malformed(job_id))
    }
}

/// Shorthand for a malformed-setup failure.
fn setup_malformed(job_id: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("setup_mise_malformed:{job_id}"))
}

/// True for catalog version spellings (`2026.9.16`); never `latest`.
fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}

/// True for lowercase hex (any length; callers fix the length).
fn is_lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
