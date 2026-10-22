//! Acquisition of the source admission artifact for a preview release.

use std::{path::Path, time::Duration};

use velnor_actions_contract::{
    PackageReleasePolicy, canonical_json_bytes, package_release::PackageReleaseChannel,
    parse_package_release_policy,
};
use velnor_actions_mise::CheckDeadline;

use super::super::github::{GitHubApi, sha256};
use super::{
    Admission, MAX_ADMISSION_BYTES, SourceAdmissionBinding, append_output, authenticate,
    lower_sha256, positive_u64, required, resolve_policy_at_source, runner_root,
    validate_admission,
};
use crate::{OrchestratorError, internal::internal};

/// Acquire the exact source admission artifact for this current workflow attempt.
/// # Errors
/// Rejects any artifact, digest, source, context, policy, or plan substitution.
pub fn acquire_preview_source_from_env() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("release_policy_deadline"))?;
    deadline
        .scope(|| acquire_source(deadline))
        .map_err(|_| internal("release_policy_deadline"))?
}

fn acquire_source(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let temp = runner_root()?;
    let policy = parse_package_release_policy(required("VELNOR_RELEASE_POLICY")?.as_bytes())
        .map_err(|_| internal("release_policy_invalid"))?;
    if policy.channel != PackageReleaseChannel::Preview {
        return Err(internal("release_policy_requires_preview_admission"));
    }
    let binding = source_admission_binding()?;
    let api = GitHubApi::new(policy.repository.clone());
    let artifact = source_admission_artifact(&api, &binding, &policy)?;
    let bytes = download_source_admission(&api, &binding, &artifact, &temp, deadline)?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_ADMISSION_BYTES)
            .map_err(|_| internal("release_policy_admission_json"))?;
    let admission: Admission =
        serde_json::from_value(value).map_err(|_| internal("release_policy_admission_schema"))?;
    let plan = resolve_policy_at_source(&policy, &admission.context.source_sha)?;
    validate_admission(&admission, &policy, &plan, &bytes, deadline)?;
    authenticate(&plan, &admission.context)?;
    validate_acquired_context(&admission, &binding, &bytes, &temp)?;
    let policy_bytes = policy
        .canonical_bytes()
        .map_err(|_| internal("release_policy_encoding"))?;
    let plan_bytes = plan
        .canonical_bytes()
        .map_err(|_| internal("release_plan_encoding"))?;
    install_acquired_source(&temp, &bytes, &policy_bytes, &plan_bytes)?;
    append_output(&format!("admission_digest={}\n", sha256(&bytes)))
}

fn source_admission_binding() -> Result<SourceAdmissionBinding, OrchestratorError> {
    let artifact_digest = required("VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_DIGEST")?;
    let inner_digest = required("VELNOR_RELEASE_SOURCE_ADMISSION_SHA256")?;
    if !lower_sha256(&artifact_digest) || !lower_sha256(&inner_digest) {
        return Err(internal("release_policy_source_artifact_digest"));
    }
    Ok(SourceAdmissionBinding {
        run_id: positive_u64("GITHUB_RUN_ID")?,
        attempt: u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
            .map_err(|_| internal("release_policy_admission_context_environment"))?,
        artifact_id: positive_u64("VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_ID")?,
        artifact_digest,
        inner_digest,
        repository_id: positive_u64("GITHUB_REPOSITORY_ID")?,
        authority: required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?,
    })
}

fn source_admission_artifact(
    api: &GitHubApi,
    binding: &SourceAdmissionBinding,
    policy: &PackageReleasePolicy,
) -> Result<super::super::github::WorkflowArtifact, OrchestratorError> {
    let mut matching = None;
    for artifact in api.workflow_artifacts(binding.run_id)? {
        if artifact.id == binding.artifact_id && matching.replace(artifact).is_some() {
            return Err(internal("release_policy_source_artifact_id"));
        }
    }
    let artifact = matching.ok_or_else(|| internal("release_policy_source_artifact_id"))?;
    let run = artifact
        .workflow_run
        .as_ref()
        .ok_or_else(|| internal("release_policy_source_artifact_run"))?;
    if artifact.name
        != format!(
            "velnor-preview-source-r{}-a{}",
            binding.run_id, binding.attempt
        )
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > 32 * 1024 * 1024
        || artifact.digest.as_deref()
            != Some(format!("sha256:{}", binding.artifact_digest).as_str())
        || run.id != binding.run_id
        || run.repository_id != binding.repository_id
        || run.head_sha != binding.authority
        || run.head_sha != required("GITHUB_SHA")?
        || run.head_branch != policy.default_branch
    {
        return Err(internal("release_policy_source_artifact_identity"));
    }
    Ok(artifact)
}

fn download_source_admission(
    api: &GitHubApi,
    binding: &SourceAdmissionBinding,
    artifact: &super::super::github::WorkflowArtifact,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    deadline
        .remaining()
        .map_err(|_| internal("release_policy_deadline"))?;
    let zip = api.download_artifact_archive_bounded(
        binding.artifact_id,
        usize::try_from(artifact.size_in_bytes)
            .map_err(|_| internal("release_policy_source_artifact_size"))?,
    )?;
    if zip.len() as u64 != artifact.size_in_bytes || sha256(&zip) != binding.artifact_digest {
        return Err(internal("release_policy_source_artifact_digest"));
    }
    let extracted = temp.join("preview-source-admission-acquired");
    crate::exclusive_write::create_dir_no_symlink(temp, &extracted)?;
    super::super::artifact_zip::extract_in_scope(
        &zip,
        &extracted,
        &[super::super::artifact_zip::ExpectedZipEntry {
            name: "preview-source-admission.json".to_owned(),
            size: None,
            sha256: Some(binding.inner_digest.clone()),
            max_bytes: u64::try_from(MAX_ADMISSION_BYTES)
                .map_err(|_| internal("release_policy_admission_size"))?,
            unix_permissions: Some(0o644),
        }],
    )?;
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes(
        &extracted.join("preview-source-admission.json"),
        MAX_ADMISSION_BYTES as u64,
    )
    .map_err(|_| internal("release_policy_source_admission_read"))?;
    if sha256(&bytes) != binding.inner_digest {
        return Err(internal("release_policy_source_admission_digest"));
    }
    Ok(bytes)
}

fn validate_acquired_context(
    admission: &Admission,
    binding: &SourceAdmissionBinding,
    bytes: &[u8],
    temp: &Path,
) -> Result<(), OrchestratorError> {
    if admission.context.workflow_authority_sha != binding.authority
        || admission.context.source_sha != required("VELNOR_RELEASE_SOURCE_SHA")?
        || admission.context.run_id != binding.run_id
        || admission.context.run_attempt != binding.attempt
        || canonical_json_bytes(admission)
            .map_err(|_| internal("release_policy_admission_encoding"))?
            != bytes
        || required("VELNOR_REQUEST_FILE")? != temp.join("package-release-policy.json")
    {
        return Err(internal("release_policy_source_admission_context"));
    }
    Ok(())
}

fn install_acquired_source(
    temp: &Path,
    admission: &[u8],
    policy: &[u8],
    plan: &[u8],
) -> Result<(), OrchestratorError> {
    crate::exclusive_write::write_exclusive(
        &temp.join("package-release-policy.json"),
        policy,
        "release_policy",
    )?;
    crate::exclusive_write::write_exclusive(
        &temp.join("package-release-plan.json"),
        plan,
        "release_plan",
    )?;
    crate::exclusive_write::write_exclusive(
        &temp.join("release-policy-admission.json"),
        admission,
        "release_policy_admission",
    )?;
    Ok(())
}
