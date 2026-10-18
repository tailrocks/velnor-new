//! Unprivileged workflow entrypoint for native preview production.

use std::{env, fs, path::PathBuf};

use serde::Serialize;
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes, config::RustBinaryPreviewConfig,
    package_release::PackageReleaseChannel, parse_package_release_plan,
    preview_identity::PreviewIdentity,
};
use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError,
    exclusive_write::{create_dir_no_symlink, write_exclusive},
    internal::internal,
    package_release_artifacts::{PackageOutputIdentity, verify_package_output},
    prepare::prepare,
    preview_producer::{PreviewProducerProof, execute_preview_task},
    retrieve_reports::{MAX_RETRIEVE_PLAN_BYTES, read_staged_bytes},
};

/// Private operation tag used by the generated preview workflow.
pub const EXECUTE_PREVIEW_PRODUCER_OP: &str = "execute-preview-producer-v1";
const PLAN_NAME: &str = "package-release-plan.json";
const PROOF_NAME: &str = "preview-proof.json";

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PreviewExecutionProof<'a> {
    schema: &'static str,
    plan: &'a PackageReleasePlan,
    producer: &'a PreviewProducerProof,
    run_id: String,
    run_attempt: String,
    staged_assets: Vec<StagedAssetProof>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct StagedAssetProof {
    name: String,
    sha256: String,
    size: u64,
}

/// Execute the workflow-authorized producer and admit its exact output inventory.
///
/// The plan contains declarations only. Task configuration is reloaded from the
/// exact checked-out source and compared with the plan before task execution.
/// Raw upload material is copied to `RUNNER_TEMP/velnor/preview-output`; the
/// verifier stages a separate locally checked set under `preview-verified`.
/// # Errors
/// Fails closed on context, source, plan, inventory, task, artifact, or path errors.
pub fn execute_preview_producer() -> Result<(), OrchestratorError> {
    let (workspace, temp_root) = workflow_context()?;
    let expected_plan = temp_root.join(PLAN_NAME);
    let request = env::var_os("VELNOR_REQUEST_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| internal("preview_request_missing"))?;
    if request != expected_plan {
        return Err(internal("preview_request_path_mismatch"));
    }
    let plan_bytes = read_staged_bytes(&request, MAX_RETRIEVE_PLAN_BYTES)
        .map_err(|_| internal("preview_plan_read"))?;
    let plan =
        parse_package_release_plan(&plan_bytes).map_err(|_| internal("preview_plan_invalid"))?;
    validate_workflow_context(&plan)?;

    let prep = prepare(&workspace)?;
    let preview = source_preview(&prep, &plan)?;

    let source_sha = required_env("GITHUB_SHA")?;
    let source_ref = required_env("GITHUB_REF")?;
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(
        u64::from(preview.producer.timeout_minutes) * 60,
    ))
    .map_err(|error| internal(&error.to_string()))?;
    let count = crate::preview_producer::source_commit_count(&prep.root, &source_sha, deadline)?;
    if required_env("VELNOR_RELEASE_COMMIT_COUNT")?
        .parse::<u64>()
        .ok()
        != Some(count)
    {
        return Err(internal("preview_eligibility_commit_count_mismatch"));
    }
    let identity = PreviewIdentity::derive(&plan.base_version, &source_sha, count)
        .map_err(|_| internal("preview_identity_invalid"))?;

    let producer = execute_preview_task(
        &prep.root,
        &temp_root,
        &identity,
        &source_ref,
        &preview,
        &prep.config.qualified_tools,
    )?;
    let producer_root = prep.root.join(&preview.producer.output_directory);
    let manifest_relative = preview
        .producer
        .manifest
        .strip_prefix(&format!("{}/", preview.producer.output_directory))
        .ok_or_else(|| internal("preview_manifest_not_under_output"))?;
    let verified_dir = temp_root.join("preview-verified");
    let verified = verify_package_output(
        &producer_root,
        manifest_relative,
        &verified_dir,
        &plan.products,
        &preview.companions,
        PackageOutputIdentity {
            source_commit: &source_sha,
            source_ref: &source_ref,
            version: &identity.version,
        },
    )?;

    let staged_assets = stage_raw_output(&temp_root, &producer_root, manifest_relative, verified)?;
    let proof = PreviewExecutionProof {
        schema: "velnor.preview-producer-proof.v1",
        plan: &plan,
        producer: &producer,
        run_id: required_env("GITHUB_RUN_ID")?,
        run_attempt: required_env("GITHUB_RUN_ATTEMPT")?,
        staged_assets,
    };
    let proof_bytes = canonical_json_bytes(&proof).map_err(|_| internal("preview_proof_encode"))?;
    write_exclusive(&temp_root.join(PROOF_NAME), &proof_bytes, "preview_proof")?;
    Ok(())
}

fn source_preview(
    prep: &crate::GenerationPreparation,
    plan: &PackageReleasePlan,
) -> Result<RustBinaryPreviewConfig, OrchestratorError> {
    let release = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .ok_or_else(|| internal("preview_config_missing"))?;
    let preview = &release.preview;
    preview.validate(".velnor/config.toml")?;
    if !preview.enabled || crate::binary_preview_emit::release_plan(prep, release)? != *plan {
        return Err(internal("preview_plan_config_mismatch"));
    }
    Ok(preview.clone())
}

fn stage_raw_output(
    temp_root: &std::path::Path,
    producer_root: &std::path::Path,
    manifest_relative: &str,
    verified: crate::package_release_artifacts::VerifiedPackageOutput,
) -> Result<Vec<StagedAssetProof>, OrchestratorError> {
    let raw_dir = temp_root.join("preview-output");
    if fs::symlink_metadata(&raw_dir).is_ok() {
        return Err(internal("preview_raw_output_preexisting"));
    }
    create_dir_no_symlink(temp_root, &raw_dir)?;
    let manifest_bytes = fs::read(producer_root.join(manifest_relative))
        .map_err(|_| internal("preview_manifest_read"))?;
    write_exclusive(
        &raw_dir.join("package-output.json"),
        &manifest_bytes,
        "preview_handoff",
    )?;
    let mut staged_assets = Vec::with_capacity(verified.artifacts.len());
    for artifact in verified.artifacts {
        let bytes =
            fs::read(&artifact.path).map_err(|_| internal("preview_verified_asset_read"))?;
        write_exclusive(&raw_dir.join(&artifact.name), &bytes, "preview_raw_asset")?;
        staged_assets.push(StagedAssetProof {
            name: artifact.name,
            sha256: artifact.sha256,
            size: artifact.size,
        });
    }
    staged_assets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(staged_assets)
}

fn workflow_context() -> Result<(PathBuf, PathBuf), OrchestratorError> {
    if required_env("GITHUB_EVENT_NAME")? != "push" {
        return Err(internal("preview_event_not_push"));
    }
    let workspace = PathBuf::from(required_env("GITHUB_WORKSPACE")?);
    let temp = PathBuf::from(required_env("RUNNER_TEMP")?);
    let workspace = workspace
        .canonicalize()
        .map_err(|_| internal("preview_workspace"))?;
    let temp = temp
        .canonicalize()
        .map_err(|_| internal("preview_runner_temp"))?;
    let temp_root = temp.join("velnor");
    create_dir_no_symlink(&temp, &temp_root)?;
    Ok((workspace, temp_root))
}

fn validate_workflow_context(plan: &PackageReleasePlan) -> Result<(), OrchestratorError> {
    if plan.channel != PackageReleaseChannel::Preview {
        return Err(internal("preview_plan_channel_mismatch"));
    }
    let repository = required_env("GITHUB_REPOSITORY")?;
    let reference = required_env("GITHUB_REF")?;
    let sha = required_env("GITHUB_SHA")?;
    let workflow_sha = required_env("GITHUB_WORKFLOW_SHA")?;
    let workflow_ref = required_env("GITHUB_WORKFLOW_REF")?;
    if !workflow_identity_matches(
        plan,
        &repository,
        &reference,
        &sha,
        &workflow_sha,
        &workflow_ref,
    ) {
        return Err(internal("preview_workflow_context_mismatch"));
    }
    for name in ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"] {
        let value = required_env(name)?;
        if value.parse::<u64>().ok().is_none_or(|number| number == 0) {
            return Err(internal("preview_run_identity_invalid"));
        }
    }
    Ok(())
}

fn workflow_identity_matches(
    plan: &PackageReleasePlan,
    repository: &str,
    reference: &str,
    sha: &str,
    workflow_sha: &str,
    workflow_ref: &str,
) -> bool {
    repository.eq_ignore_ascii_case(&plan.repository)
        && reference == format!("refs/heads/{}", plan.default_branch)
        && sha == workflow_sha
        && workflow_ref
            == format!(
                "{repository}/{}@{reference}",
                PackageReleaseChannel::Preview.workflow_path()
            )
        && is_full_sha(sha)
}

fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("preview_context_missing"))
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "preview_producer_entry_tests.rs"]
mod tests;
