//! Unprivileged workflow entrypoint for native preview production.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Serialize;
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes, config::RustBinaryPreviewConfig,
    package_release::PackageReleaseChannel, parse_package_release_plan,
    preview_identity::PreviewIdentity,
};
use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError,
    exclusive_write::{create_dir_no_symlink, write_exclusive_until},
    internal::internal,
    package_release_artifacts::{PackageOutputIdentity, verify_package_output},
    prepare::prepare_with_config,
    preview_producer::{PreviewProducerProof, execute_preview_task},
    retrieve_reports::{MAX_RETRIEVE_PLAN_BYTES, staged_reads::read_staged_bytes_until},
};

#[path = "preview_producer_proof.rs"]
mod proof;
use proof::{ProofGuard, publish_proof, with_proof_ownership};

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
    let started = Instant::now();
    let maximum = CheckDeadline::from_start_with_cleanup_reserve(
        started,
        Duration::from_secs(u64::from(velnor_actions_contract::JobTimeout::MAX_MINUTES) * 60),
    )
    .map_err(|error| internal(&error.to_string()))?;
    with_proof_ownership(|proof| {
        maximum
            .scope(|| {
                let (workspace, temp_root) = workflow_context()?;
                let config = crate::config::load_config(&workspace)?;
                let preview = config
                    .stacks
                    .rust
                    .as_ref()
                    .ok_or_else(|| internal("preview_config_missing"))?;
                let deadline = CheckDeadline::from_start_with_cleanup_reserve(
                    started,
                    Duration::from_secs(
                        u64::from(preview.binary_release.preview.producer.timeout_minutes) * 60,
                    ),
                )
                .map_err(|error| internal(&error.to_string()))?;
                deadline
                    .scope(|| {
                        execute_preview_producer_until(
                            &workspace, &temp_root, deadline, proof, config,
                        )
                    })
                    .map_err(|error| internal(&error.to_string()))?
            })
            .map_err(|error| internal(&error.to_string()))?
    })
}

fn execute_preview_producer_until(
    workspace: &Path,
    temp_root: &Path,
    deadline: CheckDeadline,
    proof: &mut ProofGuard,
    config: velnor_actions_contract::VelnorConfig,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    let expected_plan = temp_root.join(PLAN_NAME);
    let request = env::var_os("VELNOR_REQUEST_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| internal("preview_request_missing"))?;
    if request != expected_plan {
        return Err(internal("preview_request_path_mismatch"));
    }
    let plan_bytes = read_staged_bytes_until(&request, MAX_RETRIEVE_PLAN_BYTES, || {
        read_checkpoint(deadline)
    })
    .map_err(|_| internal("preview_plan_read"))?;
    let plan =
        parse_package_release_plan(&plan_bytes).map_err(|_| internal("preview_plan_invalid"))?;
    validate_workflow_context(&plan)?;

    let prep = prepare_with_config(workspace, config)?;
    checkpoint(deadline)?;
    let preview = source_preview(&prep, &plan)?;

    let source_sha = required_env("GITHUB_SHA")?;
    let source_ref = required_env("GITHUB_REF")?;
    let identity = derive_identity(&prep.root, &source_sha, &plan, deadline)?;

    let producer = execute_preview_task(
        &prep.root,
        temp_root,
        &identity,
        &source_ref,
        &preview,
        &prep.config.qualified_tools,
        deadline,
    )?;
    let producer_root = prep.root.join(&preview.producer.output_directory);
    let manifest_relative = preview
        .producer
        .manifest
        .strip_prefix(&format!("{}/", preview.producer.output_directory))
        .ok_or_else(|| internal("preview_manifest_not_under_output"))?;
    let verified_dir = temp_root.join("preview-verified");
    checkpoint(deadline)?;
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

    checkpoint(deadline)?;
    let staged_assets = stage_raw_output(
        temp_root,
        &producer_root,
        manifest_relative,
        verified,
        deadline,
    )?;
    publish_execution_proof(temp_root, &plan, &producer, staged_assets, deadline, proof)
}

fn derive_identity(
    root: &Path,
    source_sha: &str,
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<PreviewIdentity, OrchestratorError> {
    let count = crate::preview_producer::source_commit_count(root, source_sha, deadline)?;
    if required_env("VELNOR_RELEASE_COMMIT_COUNT")?
        .parse::<u64>()
        .ok()
        != Some(count)
    {
        return Err(internal("preview_eligibility_commit_count_mismatch"));
    }
    PreviewIdentity::derive(&plan.base_version, source_sha, count)
        .map_err(|_| internal("preview_identity_invalid"))
}

fn publish_execution_proof(
    temp_root: &Path,
    plan: &PackageReleasePlan,
    producer: &PreviewProducerProof,
    staged_assets: Vec<StagedAssetProof>,
    deadline: CheckDeadline,
    guard: &mut ProofGuard,
) -> Result<(), OrchestratorError> {
    let proof = PreviewExecutionProof {
        schema: "velnor.preview-producer-proof.v1",
        plan,
        producer,
        run_id: required_env("GITHUB_RUN_ID")?,
        run_attempt: required_env("GITHUB_RUN_ATTEMPT")?,
        staged_assets,
    };
    let proof_bytes = canonical_json_bytes(&proof).map_err(|_| internal("preview_proof_encode"))?;
    publish_proof(&temp_root.join(PROOF_NAME), &proof_bytes, deadline, guard)?;
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
    deadline: CheckDeadline,
) -> Result<Vec<StagedAssetProof>, OrchestratorError> {
    checkpoint(deadline)?;
    let raw_dir = temp_root.join("preview-output");
    if fs::symlink_metadata(&raw_dir).is_ok() {
        return Err(internal("preview_raw_output_preexisting"));
    }
    create_dir_no_symlink(temp_root, &raw_dir)?;
    let manifest_bytes = read_staged_bytes_until(
        &producer_root.join(manifest_relative),
        MAX_RETRIEVE_PLAN_BYTES,
        || read_checkpoint(deadline),
    )
    .map_err(|_| internal("preview_manifest_read"))?;
    write_exclusive_until(
        &raw_dir.join("package-output.json"),
        &manifest_bytes,
        "preview_handoff",
        || checkpoint(deadline),
    )?;
    let mut staged_assets = Vec::with_capacity(verified.artifacts.len());
    for artifact in verified.artifacts {
        let bytes =
            read_staged_bytes_until(&artifact.path, artifact.size, || read_checkpoint(deadline))
                .map_err(|_| internal("preview_verified_asset_read"))?;
        write_exclusive_until(
            &raw_dir.join(&artifact.name),
            &bytes,
            "preview_raw_asset",
            || checkpoint(deadline),
        )?;
        staged_assets.push(StagedAssetProof {
            name: artifact.name,
            sha256: artifact.sha256,
            size: artifact.size,
        });
    }
    checkpoint(deadline)?;
    staged_assets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(staged_assets)
}

fn read_checkpoint(deadline: CheckDeadline) -> Result<(), &'static str> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| "preview_deadline_exhausted")
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    read_checkpoint(deadline).map_err(internal)
}

fn is_preview_event(event: &str) -> bool {
    event == velnor_actions_contract::package_release::NativePreviewEvent::WorkflowRun.as_str()
}

fn workflow_context() -> Result<(PathBuf, PathBuf), OrchestratorError> {
    if !is_preview_event(&required_env("GITHUB_EVENT_NAME")?) {
        return Err(internal("preview_event_not_workflow_run"));
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
