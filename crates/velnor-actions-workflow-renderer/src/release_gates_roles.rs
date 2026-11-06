// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ReleaseConfigBinding, ReleaseJobSpec, RenderError, Step, StepKind, check_checkout_shape,
    check_runtime_steps, forbid_inputs_interpolation, forbid_registry_token,
    forbid_unexpected_secrets, release_pr_artifact_step, release_pr_candidate_step,
    release_pr_writer_step,
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
