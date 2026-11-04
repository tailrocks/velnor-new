// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    COORDINATE_CONDITION, REJECT_MODE_CONDITION, ReleaseJobSpec, ReleaseRole, RenderError, Step,
    StepId, StepKind, StepRole, check_checkout_shape, forbid_inputs_interpolation,
    forbid_registry_token, forbid_unexpected_secrets, is_forge_binding,
};

pub(super) fn check_reconcile(
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

fn runtime_step_names(role: ReleaseRole) -> &'static [&'static str] {
    match role {
        ReleaseRole::Preflight => &[
            "Acquire Velnor",
            "Checkout",
            "Release source-admit",
            "Checkout exact source",
            "Setup Mise",
            "Prepare pinned tools",
            "Verify Rust channel manifest",
            "Acquire cargo-semver-checks",
            "Release candidate-receipt",
            "Upload release receipt",
            "Archive release Rust toolchain",
            "Upload release Rust toolchain",
        ],
        ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap => &[
            "Acquire Velnor",
            "Checkout",
            "Release source-require",
            "Checkout exact source",
            "Release toolchain-acquire",
            "Verify Rust channel manifest",
            "Acquire cargo-semver-checks",
            "Release receipt-acquire",
            "Release verify-admission",
            "Reject unsupported release action",
            "Acquire release-plz",
            "Release journal-start",
            "Release journal-coordinate",
        ],
        ReleaseRole::Reconcile => &[
            "Acquire Velnor",
            "Checkout",
            "Release source-require",
            "Checkout exact source",
            "Release toolchain-acquire",
            "Verify Rust channel manifest",
            "Release reconciliation-receipt-acquire",
            "Release reconcile",
            "Upload reconciliation record",
        ],
        ReleaseRole::Preparation | ReleaseRole::PrCreate => &[],
    }
}

pub(super) fn check_runtime_steps(
    id: &str,
    job: &ReleaseJobSpec,
    version: &str,
    plz_version: Option<&str>,
) -> Result<(), RenderError> {
    let expected = runtime_step_names(job.role);
    if job.steps.len() != expected.len()
        || job
            .steps
            .iter()
            .zip(expected)
            .any(|(step, name)| step.name != *name)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_step_sequence:{id}"
        )));
    }
    let velnor_actions_contract::StepKind::Shell { env, .. } = &job.steps[0].kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    };
    let source_sha = env
        .get("VELNOR_GENERATOR_SOURCE_SHA")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    let manifest_sha = env
        .get("VELNOR_GENERATOR_MANIFEST_SHA256")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    if job.steps[0]
        != crate::release_generator_bootstrap::acquire_step(
            version,
            &job.runs_on,
            source_sha,
            manifest_sha,
        )?
        || job.steps[0].role != Some(velnor_actions_contract::StepRole::AcquireVelnor)
        || job.steps[2].id != Some(velnor_actions_contract::StepId::ReleaseAdmission)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_source_admission:{id}"
        )));
    }
    for (index, step) in job.steps.iter().enumerate() {
        check_runtime_condition(id, index, step, job.role)?;
        check_runtime_payload(id, step, version, plz_version, &job.runs_on, job.role)?;
    }
    Ok(())
}

fn check_runtime_condition(
    id: &str,
    index: usize,
    step: &Step,
    _role: ReleaseRole,
) -> Result<(), RenderError> {
    let expected = match step.name.as_str() {
        "Setup Mise" | "Prepare pinned tools" | "Verify Rust channel manifest" => None,
        "Reject unsupported release action" => Some(REJECT_MODE_CONDITION),
        "Release journal-start" | "Release journal-coordinate" | "Acquire release-plz" => {
            Some(COORDINATE_CONDITION)
        }
        _ => (index >= 3).then_some("steps.release-admission.outputs.eligible == 'true'"),
    };
    if step.condition.as_deref() != expected {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_step_condition:{id}:{}",
            step.name
        )));
    }
    Ok(())
}

fn check_runtime_payload(
    id: &str,
    step: &Step,
    version: &str,
    plz_version: Option<&str>,
    runner: &str,
    _role: ReleaseRole,
) -> Result<(), RenderError> {
    if step.name == "Acquire cargo-semver-checks" {
        let mut expected = crate::release_plz_acquire::acquire_semver_step(runner)?;
        expected.condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
        if *step != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "cargo_semver_acquire:{id}"
            )));
        }
    }
    if step.name == "Acquire release-plz" {
        let version = plz_version
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_plz_version:{id}")))?;
        let mut expected = crate::release_plz_acquire::acquire_step(version, runner)?;
        expected.condition = Some(COORDINATE_CONDITION.to_owned());
        if *step != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_plz_acquire:{id}"
            )));
        }
    }
    if step.name == "Release verify-admission"
        && step.id != Some(velnor_actions_contract::StepId::ReleaseVerification)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_verification_output:{id}"
        )));
    }
    if step.name == "Reject unsupported release action"
        && !matches!(&step.kind, StepKind::Shell { run, env } if run == &["false"] && env.is_empty())
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_unknown_action:{id}"
        )));
    }
    if let Some(operation) = match step.name.as_str() {
        "Release source-admit" => Some("source-admit"),
        "Release source-require" => Some("source-require"),
        "Release toolchain-acquire" => Some("toolchain-acquire"),
        "Release receipt-acquire" => Some("receipt-acquire"),
        "Release reconciliation-receipt-acquire" => Some("reconciliation-receipt-acquire"),
        "Release candidate-receipt" => Some("candidate-receipt"),
        "Release verify-admission" => Some("verify-admission"),
        "Release journal-start" => Some("journal-start"),
        "Release journal-coordinate" => Some("journal-coordinate"),
        "Release reconcile" => Some("reconcile"),
        _ => None,
    } {
        let StepKind::Shell { run, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_command:{id}"
            )));
        };
        if run.len() != 3
            || run[0] != format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}")
            || run[1] != "crate-release"
            || run[2] != operation
            || !env.iter().any(|(key, value)| is_forge_binding(key, value))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_command:{id}"
            )));
        }
    }
    check_receipt_step(id, step)
}

fn check_receipt_step(id: &str, step: &Step) -> Result<(), RenderError> {
    if !matches!(
        step.name.as_str(),
        "Upload release receipt" | "Upload publication record" | "Upload reconciliation record"
    ) {
        return Ok(());
    }
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_receipt:{id}"
        )));
    };
    let expected_action = crate::steps::UPLOAD_ARTIFACT_USES;
    let (expected_artifact, expected_path) = match step.name.as_str() {
        "Upload publication record" => (
            "velnor-crate-publication-started",
            "${{ runner.temp }}/velnor-crate-release-publication-started.json",
        ),
        "Upload reconciliation record" => (
            "velnor-crate-release-reconciled",
            "${{ runner.temp }}/velnor-crate-release-reconciled.json",
        ),
        "Upload release receipt" => (
            "velnor-release-receipt-${{ github.run_id }}-${{ github.run_attempt }}",
            "${{ runner.temp }}/velnor-crate-release-receipt.json",
        ),
        _ => {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_receipt:{id}"
            )));
        }
    };
    if uses != expected_action
        || !env.is_empty()
        || with.get("name").map(String::as_str) != Some(expected_artifact)
        || with.get("path").map(String::as_str) != Some(expected_path)
        || with.len() != 4
        || with.get("if-no-files-found").map(String::as_str) != Some("error")
        || with.get("retention-days").map(String::as_str) != Some("7")
        || (step.name == "Upload release receipt"
            && (step.id != Some(StepId::ReleaseReceiptUpload)
                || step.role != Some(StepRole::ReleaseReceiptUpload)))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_receipt:{id}"
        )));
    }
    Ok(())
}
