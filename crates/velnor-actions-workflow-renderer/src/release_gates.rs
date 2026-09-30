//! Release step-content gates: authority separation and argv safety.
//!
//! Permissions prove the grant; these gates prove the steps honor it: no
//! secrets outside the bootstrap env binding, no registry token on the
//! OIDC path, exact-source checkouts, explicit config binding, and no
//! verification bypasses or dispatch-input interpolation.

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError, commands,
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
    steps,
};

/// Explicit config paths the publish argv must reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseConfigBinding<'a> {
    /// Generated effective config path (OIDC publish binds this).
    pub effective: &'a str,
    /// Generated bootstrap-only config path (bootstrap publish binds this).
    pub bootstrap: &'a str,
}

/// Validate every step plus the role authority boundary.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid steps or boundary violations.
pub fn check_release_jobs(
    spec: &ReleaseWorkflowSpec,
    binding: &ReleaseConfigBinding<'_>,
) -> Result<(), RenderError> {
    for (id, job) in &spec.jobs {
        check_job_steps(id, job)?;
        match job.role {
            ReleaseRole::Preparation => check_preparation(id, job)?,
            ReleaseRole::Preflight => {
                check_preflight(id, job, &spec.bootstrap.source_sha)?;
            }
            ReleaseRole::PublishOidc => {
                check_publish_oidc(id, job, &spec.bootstrap.source_sha, binding.effective)?;
            }
            ReleaseRole::PublishBootstrap => {
                check_publish_bootstrap(id, job, &spec.bootstrap.source_sha, binding.bootstrap)?;
            }
            ReleaseRole::Reconcile => check_reconcile(id, job)?,
        }
    }
    Ok(())
}

/// Validate every step payload (pinned actions, fixed argv/env).
fn check_job_steps(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    for step in &job.steps {
        match &step.kind {
            StepKind::Action { uses, with } => {
                steps::validate_uses(uses)?;
                for entry in with.keys().chain(with.values()) {
                    steps::scan_for_private_subcommands(entry)?;
                }
            }
            StepKind::Shell { run, env } => {
                commands::validate_command_argv(run)?;
                commands::validate_env(env)?;
                for key in env.keys() {
                    if key.starts_with("VELNOR_MATRIX_") {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "release_matrix_fanout:{id}"
                        )));
                    }
                }
            }
            StepKind::Internal { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_internal_op:{id}"
                )));
            }
        }
    }
    Ok(())
}

/// All scannable step payload texts (argv, env, action inputs).
fn payload_texts(step: &Step) -> Vec<&str> {
    match &step.kind {
        StepKind::Action { uses, with } => std::iter::once(uses.as_str())
            .chain(with.keys().map(String::as_str))
            .chain(with.values().map(String::as_str))
            .collect(),
        StepKind::Shell { run, env } => run
            .iter()
            .map(String::as_str)
            .chain(env.keys().map(String::as_str))
            .chain(env.values().map(String::as_str))
            .collect(),
        StepKind::Internal { .. } => Vec::new(),
    }
}

/// Reject one substring in every payload text of a job's steps.
fn forbid_substring(id: &str, steps: &[Step], needle: &str, code: &str) -> Result<(), RenderError> {
    for step in steps {
        if payload_texts(step).iter().any(|text| text.contains(needle)) {
            return Err(RenderError::InvalidWorkflow(format!("{code}:{id}")));
        }
    }
    Ok(())
}

/// Reject secret handles outside the bootstrap env binding.
fn forbid_secrets(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(id, steps, "secrets.", "secret_outside_bootstrap")
}

/// Reject registry-token handles outside the bootstrap env binding.
fn forbid_registry_token(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(
        id,
        steps,
        "CARGO_REGISTRY_TOKEN",
        "registry_token_outside_bootstrap",
    )
}

/// Reject event-input interpolation in steps (inputs live in `if:` only).
fn forbid_inputs_interpolation(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(id, steps, "github.event", "dispatch_input_in_steps")?;
    forbid_substring(id, steps, "inputs.", "dispatch_input_in_steps")
}

/// Require credential-free checkouts on every release checkout step.
fn check_checkout_hygiene(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Action { uses, with } = &step.kind
            && uses.starts_with("actions/checkout@")
            && with.get("persist-credentials").is_none_or(|v| v != "false")
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "checkout_with_credentials:{id}"
            )));
        }
    }
    Ok(())
}

/// True when a checkout step pins `ref` to the exact approved SHA.
fn has_exact_checkout(steps: &[Step], sha: &str) -> bool {
    steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, with }
            if uses.starts_with("actions/checkout@")
                && with.get("ref").is_some_and(|value| value == sha))
    })
}

/// Require an exact-source checkout (no fallback to unverified checkouts).
fn require_exact_checkout(id: &str, steps: &[Step], sha: &str) -> Result<(), RenderError> {
    if has_exact_checkout(steps, sha) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "checkout_without_exact_source:{id}"
        )))
    }
}

/// Preparation: GitHub authority only, no registry token or secrets.
fn check_preparation(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    forbid_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_hygiene(id, &job.steps)
}

/// Preflight: read-only validation over the exact approved source.
fn check_preflight(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_hygiene(id, &job.steps)?;
    require_exact_checkout(id, &job.steps, sha)
}

/// True when argv carries an adjacent `--config <path>` pair.
fn has_config_binding(steps: &[Step], path: &str) -> bool {
    steps.iter().any(|step| {
        if let StepKind::Shell { run, .. } = &step.kind {
            run.windows(2)
                .any(|pair| pair[0] == "--config" && pair[1] == path)
        } else {
            false
        }
    })
}

/// Require the explicit generated-config argument on publish argv.
fn require_config_binding(id: &str, steps: &[Step], path: &str) -> Result<(), RenderError> {
    if has_config_binding(steps, path) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "missing_config_binding:{id}"
        )))
    }
}

/// Argv fragments that would bypass Cargo verification.
const VERIFY_BYPASS_TOKENS: &[&str] = &["allow-dirty", "allow_dirty", "no-verify", "no_verify"];

/// Reject Cargo verification bypasses on publish argv.
fn forbid_verify_bypass(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Shell { run, .. } = &step.kind {
            for arg in run {
                for token in VERIFY_BYPASS_TOKENS {
                    if arg.contains(token) {
                        return Err(RenderError::InvalidWorkflow(format!("verify_bypass:{id}")));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Shared publish gates: exact source, explicit config, verified Cargo.
fn check_publish_common(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    config_path: &str,
) -> Result<(), RenderError> {
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_hygiene(id, &job.steps)?;
    require_exact_checkout(id, &job.steps, sha)?;
    require_config_binding(id, &job.steps, config_path)?;
    forbid_verify_bypass(id, &job.steps)
}

/// OIDC publish: no token material anywhere, no silent token fallback.
fn check_publish_oidc(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    config_path: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, config_path)?;
    forbid_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)
}

/// True for an exact `${{ secrets.NAME }}` reference.
fn is_secret_ref(value: &str) -> bool {
    value
        .strip_prefix("${{ secrets.")
        .and_then(|rest| rest.strip_suffix(" }}"))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// Bootstrap publish: exactly one token binding, env-only, never argv.
fn check_publish_bootstrap(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    config_path: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, config_path)?;
    let mut bindings = 0_u32;
    for step in &job.steps {
        match &step.kind {
            StepKind::Shell { run, env } => {
                for arg in run {
                    if arg.contains("secrets.") {
                        return Err(RenderError::InvalidWorkflow(format!("secret_in_argv:{id}")));
                    }
                }
                for (key, value) in env {
                    if value.contains("secrets.") {
                        if key != "CARGO_REGISTRY_TOKEN" || !is_secret_ref(value) {
                            return Err(RenderError::InvalidWorkflow(format!(
                                "bootstrap_token_binding:{id}"
                            )));
                        }
                        bindings += 1;
                    }
                }
            }
            StepKind::Action { with, .. } => {
                for value in with.values() {
                    if value.contains("secrets.") {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "secret_in_action_input:{id}"
                        )));
                    }
                }
            }
            StepKind::Internal { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_internal_op:{id}"
                )));
            }
        }
    }
    if bindings == 1 {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bootstrap_token_binding:{id}"
        )))
    }
}

/// Reconciliation: independent read-only checks, no credentials.
fn check_reconcile(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    forbid_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_hygiene(id, &job.steps)
}
