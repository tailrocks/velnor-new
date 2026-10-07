//! Release step-content gates: authority separation and argv safety.
//!
//! Permissions prove the grant; these gates prove the steps honor it: the
//! forge token only as the exact `GIT_TOKEN` env binding (plus the single
//! bootstrap registry binding), no token material on the OIDC path,
//! policy-plus-source checkouts with role-exact credentials, full history
//! everywhere release-plz reads git, explicit config binding, and no
//! verification bypasses or dispatch-input interpolation.

use velnor_actions_contract_workflow::{Step, StepKind};

use crate::{
    release_checkout_gates::{check_checkout_shape, require_exact_checkout},
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
};
use velnor_actions_workflow_steps::{RenderError, commands, steps};

/// Forge-token env key every release-plz step carries.
///
/// release-plz 0.3.169 reads `--git-token` from `GIT_TOKEN`, never from
/// `GITHUB_TOKEN`; every phase (including `--dry-run`) requires it.
pub const GIT_TOKEN_ENV: &str = "GIT_TOKEN";
/// Exact secret reference bound to [`GIT_TOKEN_ENV`] (env-only, never argv).
pub const GIT_TOKEN_REF: &str = "${{ secrets.GITHUB_TOKEN }}";

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
        require_forge_binding(id, &job.steps)?;
        match job.role {
            ReleaseRole::Preparation => check_preparation(id, job, &spec.bootstrap.source_sha)?,
            ReleaseRole::Preflight => {
                check_preflight(id, job, &spec.bootstrap.source_sha)?;
            }
            ReleaseRole::PublishOidc => {
                check_publish_oidc(id, job, &spec.bootstrap.source_sha, binding.effective)?;
            }
            ReleaseRole::PublishBootstrap => {
                check_publish_bootstrap(id, job, &spec.bootstrap.source_sha, binding.bootstrap)?;
            }
            ReleaseRole::Reconcile => check_reconcile(id, job, &spec.bootstrap.source_sha)?,
        }
    }
    Ok(())
}

/// Validate every step payload (pinned actions, fixed argv/env).
fn check_job_steps(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    for step in &job.steps {
        match &step.kind {
            StepKind::Action { uses, with, env } => {
                steps::validate_uses(uses)?;
                for entry in with.keys().chain(with.values()) {
                    steps::scan_for_private_subcommands(entry)?;
                }
                commands::validate_env(env)?;
            }
            StepKind::Shell { run, env } => {
                commands::validate_command_argv(run)?;
                commands::validate_env(env)?;
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
        StepKind::Action { uses, with, env } => std::iter::once(uses.as_str())
            .chain(with.keys().map(String::as_str))
            .chain(with.values().map(String::as_str))
            .chain(env.keys().map(String::as_str))
            .chain(env.values().map(String::as_str))
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

/// True for the exact allowed forge-token env binding.
fn is_forge_binding(key: &str, value: &str) -> bool {
    key == GIT_TOKEN_ENV && value == GIT_TOKEN_REF
}

/// Reject secret handles outside the exact forge-token env binding.
///
/// Argv and action inputs carry no secrets anywhere; env values may only
/// be the exact [`GIT_TOKEN_ENV`]/[`GIT_TOKEN_REF`] pair (the bootstrap
/// registry binding is checked separately by its own role gate).
fn forbid_unexpected_secrets(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    let rejected = || RenderError::InvalidWorkflow(format!("secret_outside_bootstrap:{id}"));
    for step in steps {
        match &step.kind {
            StepKind::Action { with, .. } => {
                if with.values().any(|value| value.contains("secrets.")) {
                    return Err(rejected());
                }
            }
            StepKind::Shell { run, env } => {
                if run.iter().any(|arg| arg.contains("secrets.")) {
                    return Err(rejected());
                }
                if env
                    .iter()
                    .any(|(key, value)| value.contains("secrets.") && !is_forge_binding(key, value))
                {
                    return Err(rejected());
                }
            }
            StepKind::Internal { .. } => {}
        }
    }
    Ok(())
}

/// Require the forge-token binding on every release-plz step.
///
/// release-plz fails without `--git-token` (env `GIT_TOKEN`) in every
/// phase, so a step invoking it without the exact binding fails closed
/// here instead of at runtime.
fn require_forge_binding(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Shell { run, env } = &step.kind
            && run.iter().any(|arg| arg == "release-plz")
            && !env.iter().any(|(key, value)| is_forge_binding(key, value))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "missing_forge_binding:{id}"
            )));
        }
    }
    Ok(())
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

/// Preparation: GitHub authority only, no registry token or secrets.
fn check_preparation(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)
}

/// Preflight: read-only validation over the exact approved source.
fn check_preflight(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
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
    check_checkout_shape(id, &job.steps, job.role, sha)?;
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
    forbid_unexpected_secrets(id, &job.steps)?;
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

/// Bootstrap publish: exactly one registry binding plus the forge binding.
///
/// Both bindings are env-only, never argv; any other secret handle fails.
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
                    if !value.contains("secrets.") || is_forge_binding(key, value) {
                        continue;
                    }
                    if key != "CARGO_REGISTRY_TOKEN" || !is_secret_ref(value) {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "bootstrap_token_binding:{id}"
                        )));
                    }
                    bindings += 1;
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

/// Reconciliation: independent read-only checks, no push credentials.
fn check_reconcile(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)
}
