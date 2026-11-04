// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GIT_TOKEN_ENV, GIT_TOKEN_REF, ReleaseJobSpec, RenderError, Step, StepKind, commands,
    forbid_substring, steps,
};

pub(super) fn check_job_steps(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
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

pub(super) fn is_forge_binding(key: &str, value: &str) -> bool {
    key == GIT_TOKEN_ENV && value == GIT_TOKEN_REF
}

pub(super) fn forbid_unexpected_secrets(id: &str, steps: &[Step]) -> Result<(), RenderError> {
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
                if env.iter().any(|(key, value)| {
                    value.contains("secrets.")
                        && !is_forge_binding(key, value)
                        && !(matches!(
                            step.name.as_str(),
                            "Release journal-start" | "Release journal-coordinate"
                        ) && key == "GH_TOKEN"
                            && value == GIT_TOKEN_REF)
                }) {
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

pub(super) fn require_forge_binding(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Shell { run, env } = &step.kind
            && run
                .iter()
                .any(|arg| arg == crate::release_plz_acquire::BINARY_PATH)
            && !env.iter().any(|(key, value)| is_forge_binding(key, value))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "missing_forge_binding:{id}"
            )));
        }
    }
    Ok(())
}

pub(super) fn forbid_registry_token(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        let rejected = match &step.kind {
            StepKind::Shell { run, env } => {
                (step.role != Some(velnor_actions_contract::StepRole::VerifyRustManifest)
                    && run.iter().any(|part| part.contains("CARGO_REGISTRY_TOKEN")))
                    || env.get("CARGO_REGISTRY_TOKEN").is_some_and(|value| {
                        !value.is_empty()
                            || !matches!(
                            step.role,
                            Some(
                                velnor_actions_contract::StepRole::AcquireVelnor
                                    | velnor_actions_contract::StepRole::AcquireReleasePlz
                                    | velnor_actions_contract::StepRole::AcquireCargoSemverChecks
                                    | velnor_actions_contract::StepRole::VerifyRustManifest
                                    | velnor_actions_contract::StepRole::PreparePinnedTools
                                    | velnor_actions_contract::StepRole::ReleaseRustArchive
                            )
                        )
                    })
            }
            StepKind::Action { uses, with, env } => {
                uses.contains("CARGO_REGISTRY_TOKEN")
                    || with.iter().any(|(key, value)| {
                        key.contains("CARGO_REGISTRY_TOKEN")
                            || value.contains("CARGO_REGISTRY_TOKEN")
                    })
                    || env.get("CARGO_REGISTRY_TOKEN").is_some_and(|value| {
                        !value.is_empty()
                            || !matches!(
                            step.role,
                            Some(
                                velnor_actions_contract::StepRole::AcquireVelnor
                                    | velnor_actions_contract::StepRole::AcquireReleasePlz
                                    | velnor_actions_contract::StepRole::AcquireCargoSemverChecks
                            )
                        )
                    })
            }
            _ => true,
        };
        if rejected {
            return Err(RenderError::InvalidWorkflow(format!(
                "registry_token_outside_bootstrap:{id}"
            )));
        }
    }
    Ok(())
}

pub(super) fn forbid_inputs_interpolation(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(id, steps, "github.event", "dispatch_input_in_steps")?;
    forbid_substring(id, steps, "inputs.", "dispatch_input_in_steps")
}
