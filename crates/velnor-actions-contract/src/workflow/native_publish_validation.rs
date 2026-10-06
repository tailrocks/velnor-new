//! Fail-closed graph and step checks for isolated native attestation jobs.
use super::{NativePublishRole, invalid};
use crate::workflow::ActionOutput;
use crate::{ContractError, Job, PermissionLevel, Permissions, StepId, StepKind, WorkflowIr};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate(
    role: &NativePublishRole,
    job: &Job,
    workflow: &WorkflowIr,
) -> Result<(), ContractError> {
    identity(role)?;
    let b = role.binding();
    let expected = Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        id_token: PermissionLevel::Write,
        attestations: PermissionLevel::Write,
        ..Permissions::default()
    };
    if job.permissions.as_ref() != Some(&expected)
        || job.condition.as_deref() != Some(role.condition().as_str())
        || job.source_producer.is_some()
        || job.tool_producer.is_some()
        || job.native_pages_deploy.is_some()
        || job
            .cache_mode
            .is_some_and(|mode| mode != crate::CacheMode::Read)
        || !job.outputs.is_empty()
        || !job.needs.contains(&b.full_ci_job)
        || !job.needs.contains(&b.artifact_job)
        || workflow.triggers.merge_group
        || !workflow.triggers.pull_request_types.is_empty()
        || !workflow.triggers.push_branches.is_empty()
        || workflow.triggers.schedule.is_some()
        || workflow.triggers.push_tags != ["v[0-9]*"]
    {
        return Err(invalid("invalid_attestation_authority"));
    }
    match role {
        NativePublishRole::DesktopTagZip { environment, .. }
            if job.environment.as_ref() != Some(environment) =>
        {
            return Err(invalid("foreign_environment"));
        }
        NativePublishRole::DockerHubIndex { .. } if job.environment.is_some() => {
            return Err(invalid("foreign_environment"));
        }
        _ => {}
    }
    dependencies(role, workflow)?;
    steps(role, job)
}

fn identity(role: &NativePublishRole) -> Result<(), ContractError> {
    let b = role.binding();
    let parts = b.repository.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|p| !safe_component(p))
        || !crate::is_valid_branch_name(&b.default_branch)
        || b.default_branch.contains('\'')
        || b.full_ci_job == b.artifact_job
    {
        return Err(invalid("invalid_source_binding"));
    }
    for id in [
        &b.full_ci_job,
        &b.artifact_job,
        &b.artifact_id_output,
        &b.artifact_digest_output,
    ] {
        StepId::new(id)?;
    }
    b.admission.validate()?;
    b.receipt.validate()?;
    b.full_ci_admission.validate()?;
    super::oci::operations(role)?;
    if !b
        .attest_uses
        .strip_prefix("actions/attest@")
        .is_some_and(|sha| crate::ids::is_lower_hex_len(sha, 40))
    {
        return Err(invalid("invalid_compiled_binding"));
    }
    let mut ids = BTreeSet::new();
    for id in [&b.admission_step, &b.receipt_step, &b.attest_step] {
        id.validate()?;
        if !ids.insert(id.as_str()) {
            return Err(invalid("duplicate_role_step"));
        }
    }
    for preparation in &b.preparation {
        preparation.step_id.validate()?;
        preparation.invocation.validate()?;
        if preparation.environment.iter().any(|(key, value)| {
            matches!(key.as_str(), "GH_TOKEN" | "GITHUB_TOKEN")
                || key.starts_with("ACTIONS_ID_TOKEN_")
                || value
                    .replace("${{ runner.temp }}", "RUNNER_TEMP")
                    .contains("${{")
        }) {
            return Err(invalid("credentialed_preparation"));
        }
        if !ids.insert(preparation.step_id.as_str())
            || !super::oci::allows_preparation(
                role,
                preparation.invocation.descriptor().operation(),
            )
        {
            return Err(invalid("foreign_preparation"));
        }
    }
    super::oci::environment(role)?;
    subject(role)
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn subject(role: &NativePublishRole) -> Result<(), ContractError> {
    match role {
        NativePublishRole::DesktopTagZip {
            environment,
            subject_path,
            binding,
        } => {
            let version = format!(
                "${{{{ steps.{}.outputs.version }}}}",
                binding.admission_step.as_str()
            );
            let literal = subject_path.replace(&version, "1.2.3");
            if environment.is_empty()
                || !environment.split('/').all(safe_component)
                || subject_path.matches(&version).count() != 1
                || !literal.ends_with("-aarch64-apple-darwin.zip")
                || !literal.split('/').all(safe_component)
            {
                return Err(invalid("invalid_exact_zip_subject"));
            }
        }
        NativePublishRole::DockerHubIndex {
            subject_name,
            subject_digest_output,
            ..
        } => {
            let parts = subject_name.split('/').collect::<Vec<_>>();
            if parts.len() != 3
                || parts[0] != "docker.io"
                || parts[1..]
                    .iter()
                    .any(|part| !safe_component(part) || *part != part.to_ascii_lowercase())
            {
                return Err(invalid("invalid_dockerhub_subject"));
            }
            if subject_digest_output != "index_digest" {
                return Err(invalid("foreign_index_digest_output"));
            }
            StepId::new(subject_digest_output)?;
        }
    }
    Ok(())
}

fn dependencies(role: &NativePublishRole, workflow: &WorkflowIr) -> Result<(), ContractError> {
    let b = role.binding();
    let proof = workflow
        .jobs
        .get(&b.full_ci_job)
        .ok_or_else(|| invalid("missing_full_ci_proof"))?;
    let matching = proof.steps.iter().filter(|s| matches!(&s.kind, StepKind::SourceBoundHelper { invocation, .. } if invocation == &b.full_ci_admission)).collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].condition.is_some()
        || !matches!(&matching[0].kind, StepKind::SourceBoundHelper { env, .. } if env == &role.full_ci_environment())
        || proof.condition.as_deref() != Some(role.condition().as_str())
        || proof.native_publish.is_some()
        || proof.native_pages_deploy.is_some()
        || proof.permissions.as_ref().unwrap_or(&workflow.permissions) != &Permissions::default()
    {
        return Err(invalid("invalid_full_ci_proof"));
    }
    super::oci::version_output(role, proof)?;
    let producer = workflow
        .jobs
        .get(&b.artifact_job)
        .ok_or_else(|| invalid("missing_artifact_producer"))?;
    if producer.native_publish.is_some()
        || producer.native_pages_deploy.is_some()
        || !producer.needs.contains(&b.full_ci_job)
        || producer
            .permissions
            .as_ref()
            .unwrap_or(&workflow.permissions)
            .id_token
            != PermissionLevel::None
        || producer
            .permissions
            .as_ref()
            .unwrap_or(&workflow.permissions)
            .attestations
            != PermissionLevel::None
    {
        return Err(invalid("privileged_or_unadmitted_artifact_producer"));
    }
    let id = producer
        .outputs
        .iter()
        .find(|o| o.name == b.artifact_id_output);
    let digest = producer
        .outputs
        .iter()
        .find(|o| o.name == b.artifact_digest_output);
    if !id.zip(digest).is_some_and(|(id, digest)| {
        id.value.output == ActionOutput::ArtifactId
            && digest.value.output == ActionOutput::ArtifactDigest
            && id.value.step_id == digest.value.step_id
    }) {
        return Err(invalid("invalid_immutable_transport"));
    }
    super::super::outputs::validate_job_outputs(&producer.outputs, &producer.steps)
}

fn steps(role: &NativePublishRole, job: &Job) -> Result<(), ContractError> {
    let b = role.binding();
    if job.steps.len() != b.preparation.len() + 3 || job.steps.iter().any(|s| s.condition.is_some())
    {
        return Err(invalid("foreign_or_conditional_step"));
    }
    for (step, preparation) in job.steps.iter().zip(&b.preparation) {
        if step.id.as_ref() != Some(&preparation.step_id)
            || !matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env } if invocation == &preparation.invocation && env == &preparation.environment)
        {
            return Err(invalid("foreign_preparation_step"));
        }
    }
    let index = b.preparation.len();
    for (step, id, expected, environment) in [
        (
            &job.steps[index],
            &b.admission_step,
            &b.admission,
            role.admission_environment(),
        ),
        (
            &job.steps[index + 1],
            &b.receipt_step,
            &b.receipt,
            role.receipt_environment(),
        ),
    ] {
        if step.id.as_ref() != Some(id)
            || !matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env } if invocation == expected && env == &environment)
        {
            return Err(invalid("foreign_admission_or_receipt"));
        }
    }
    let last = &job.steps[index + 2];
    if last.id.as_ref() != Some(&b.attest_step)
        || !matches!(&last.kind, StepKind::Action { uses, with, env } if uses == &b.attest_uses && with == &attestation_inputs(role) && env.is_empty())
    {
        return Err(invalid("foreign_attestation_subject_or_action"));
    }
    Ok(())
}

pub(super) fn attestation_inputs(role: &NativePublishRole) -> BTreeMap<String, String> {
    match role {
        NativePublishRole::DesktopTagZip { subject_path, .. } => {
            BTreeMap::from([("subject-path".into(), subject_path.clone())])
        }
        NativePublishRole::DockerHubIndex {
            binding,
            subject_name,
            subject_digest_output,
            ..
        } => BTreeMap::from([
            ("subject-name".into(), subject_name.clone()),
            (
                "subject-digest".into(),
                format!(
                    "${{{{ steps.{}.outputs.{} }}}}",
                    binding.receipt_step.as_str(),
                    subject_digest_output
                ),
            ),
            ("push-to-registry".into(), "false".into()),
        ]),
    }
}
