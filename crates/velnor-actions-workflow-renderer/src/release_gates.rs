// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError, commands,
    release_checkout_gates::{check_checkout_shape, require_exact_checkout},
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
    steps,
};

#[path = "release_gates_step_text.rs"]
mod step_text;
use step_text::forbid_substring;

pub const GIT_TOKEN_ENV: &str = "GIT_TOKEN";
pub const GIT_TOKEN_REF: &str = "${{ secrets.GITHUB_TOKEN }}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseConfigBinding<'a> {
    pub effective: &'a str,
    pub bootstrap: &'a str,
}

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
            StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_declared_task:{id}"
                )));
            }
        }
    }
    Ok(())
}

fn is_forge_binding(key: &str, value: &str) -> bool {
    key == GIT_TOKEN_ENV && value == GIT_TOKEN_REF
}

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
            StepKind::TaskExecution { argv, env, .. } => {
                if argv.iter().any(|arg| arg.contains("secrets."))
                    || env.iter().any(|(key, value)| {
                        value.contains("secrets.") && !is_forge_binding(key, value)
                    })
                {
                    return Err(rejected());
                }
            }
            StepKind::NativeCheckExecution {
                check_id,
                task_id,
                sdk_version,
                ..
            } => {
                if [check_id, task_id, sdk_version]
                    .iter()
                    .any(|value| value.contains("secrets."))
                {
                    return Err(rejected());
                }
            }
        }
    }
    Ok(())
}

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

fn forbid_registry_token(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(
        id,
        steps,
        "CARGO_REGISTRY_TOKEN",
        "registry_token_outside_bootstrap",
    )
}

fn forbid_inputs_interpolation(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(id, steps, "github.event", "dispatch_input_in_steps")?;
    forbid_substring(id, steps, "inputs.", "dispatch_input_in_steps")
}

fn check_preparation(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)
}

fn check_preflight(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
    require_exact_checkout(id, &job.steps, sha)
}

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

fn require_config_binding(id: &str, steps: &[Step], path: &str) -> Result<(), RenderError> {
    if has_config_binding(steps, path) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "missing_config_binding:{id}"
        )))
    }
}

const VERIFY_BYPASS_TOKENS: &[&str] = &["allow-dirty", "allow_dirty", "no-verify", "no_verify"];

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
            StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_declared_task:{id}"
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

fn check_reconcile(id: &str, job: &ReleaseJobSpec, sha: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)
}
