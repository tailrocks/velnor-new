// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GIT_TOKEN_REF, ReleaseConfigBinding, ReleaseJobSpec, RenderError, Step, StepKind,
    check_checkout_shape, check_runtime_steps, forbid_inputs_interpolation, forbid_registry_token,
    forbid_unexpected_secrets, is_forge_binding, release_pr_artifact_step,
    release_pr_candidate_step, release_pr_writer_step,
};

pub(super) fn check_preparation(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    binding: &ReleaseConfigBinding<'_>,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha, binding.generator_version)?;
    let has_pr = !job.outputs.is_empty();
    if job.steps.len() != if has_pr { 10 } else { 8 }
        || job.steps[5]
            != crate::release_plz_acquire::acquire_step(binding.release_plz_version, &job.runs_on)?
        || job.steps[6] != crate::release_plz_acquire::acquire_semver_step(&job.runs_on)?
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_plz_acquire:{id}"
        )));
    }
    check_acquire_step(id, &job.steps[0], binding.generator_version, &job.runs_on)?;
    if has_pr {
        if job.steps[7]
            != release_pr_candidate_step(
                binding.generator_version,
                binding.rust_version,
                &job.runs_on,
            )?
            || job.steps[9] != release_pr_artifact_step()?
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_pr_preparation:{id}"
            )));
        }
    } else if job.steps[7].name != "Validate release" {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_pr_disabled:{id}"
        )));
    }
    Ok(())
}

fn check_acquire_step(
    id: &str,
    step: &Step,
    version: &str,
    runner: &str,
) -> Result<(), RenderError> {
    let StepKind::Shell { env, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    };
    let source = env
        .get("VELNOR_GENERATOR_SOURCE_SHA")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    let manifest = env
        .get("VELNOR_GENERATOR_MANIFEST_SHA256")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    if *step != crate::release_generator_bootstrap::acquire_step(version, runner, source, manifest)?
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    }
    Ok(())
}

pub(super) fn check_pr_writer(
    id: &str,
    job: &ReleaseJobSpec,
    version: &str,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    if job.steps.len() != 2 || job.steps[1] != release_pr_writer_step(version)? {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_pr_writer:{id}"
        )));
    }
    check_acquire_step(id, &job.steps[0], version, &job.runs_on)
}

pub(super) fn check_preflight(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha, version)?;
    check_runtime_steps(id, job, version, None)
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
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    if job.timeout_minutes != velnor_actions_contract::JobTimeout::CRATE_RELEASE {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_budget_timeout:{id}"
        )));
    }
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha, version)?;
    forbid_verify_bypass(id, &job.steps)?;
    check_runtime_steps(id, job, version, Some(plz_version))
}

pub(super) fn check_publish_oidc(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, version, plz_version)?;
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

pub(super) fn check_publish_bootstrap(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, version, plz_version)?;
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
                    if !value.contains("secrets.")
                        || is_forge_binding(key, value)
                        || (matches!(
                            step.name.as_str(),
                            "Release journal-start" | "Release journal-coordinate"
                        ) && key == "GH_TOKEN"
                            && value == GIT_TOKEN_REF)
                    {
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
