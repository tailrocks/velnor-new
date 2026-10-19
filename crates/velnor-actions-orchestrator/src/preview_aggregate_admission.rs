//! Privileged-job admission of authenticated aggregate bytes; executes no source.

use super::{context, handoff, leaf_manifest::PreviewAggregateManifest, read_plan};
use crate::{
    OrchestratorError,
    exclusive_write::write_exclusive_until,
    internal::internal,
    package_release_artifacts::{PackageOutputIdentity, verify_package_output},
    package_release_publish::{
        acquire_preview_aggregate,
        artifact_zip::{self, ExpectedZipEntry},
    },
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use velnor_actions_mise::CheckDeadline;

/// Admit the exact aggregate service ID before the separate, unchanged signer.
pub const ADMIT_PREVIEW_AGGREGATE_OP: &str = "admit-preview-aggregate-v1";

/// Authenticate and inspect the full union without running any repository task.
/// # Errors
/// Rejects role, source, remote authority, ZIP, native proof or full artifact drift.
pub fn admit_preview_aggregate() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(
        Instant::now(),
        Duration::from_secs(u64::from(velnor_actions_contract::JobTimeout::MAX_MINUTES) * 60),
    )
    .map_err(|error| internal(&error.to_string()))?;
    deadline
        .scope(|| admit_until(deadline))
        .map_err(|error| internal(&error.to_string()))?
}

fn admit_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    if context::required_env("GITHUB_JOB")? != "preview-signer" {
        return Err(internal("preview_signer_role"));
    }
    let temp = runner_temp()?;
    let plan = read_plan(&temp, deadline)?;
    let authenticated_context = context::workflow_context(&plan, deadline)?;
    let id = context::positive_env("VELNOR_PREVIEW_AGGREGATE_ARTIFACT_ID")?;
    let digest = context::required_env("VELNOR_PREVIEW_AGGREGATE_ARTIFACT_DIGEST")?;
    let artifact = acquire_preview_aggregate(&plan, &authenticated_context, id, &digest, deadline)?;
    let metadata = artifact_zip::read_manifest(
        &artifact.bytes,
        handoff::AGGREGATE_MANIFEST,
        handoff::MAX_NATIVE_MANIFEST,
        deadline,
    )?;
    let proof = PreviewAggregateManifest::parse(&metadata, &plan, &authenticated_context)?;
    let source_manifest = artifact_zip::read_manifest(
        &artifact.bytes,
        "package-output.json",
        crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES,
        deadline,
    )?;
    let downloaded = handoff::fresh_directory(&temp, "preview-aggregate-acquired")?;
    let entries = entries(&proof, &metadata, &source_manifest);
    artifact_zip::extract(&artifact.bytes, &downloaded, &entries, deadline)?;
    let input = handoff::fresh_directory(&temp, "preview-input")?;
    copy_full_input(&downloaded, &input, &proof, &source_manifest, deadline)?;
    let companions = plan
        .companions
        .iter()
        .map(
            |row| velnor_actions_contract::config::RustBinaryPreviewCompanion {
                id: row.id.clone(),
                path: row.path.clone(),
                asset_name: row.asset_name.clone(),
            },
        )
        .collect::<Vec<_>>();
    let verified = verify_package_output(
        &input,
        "package-output.json",
        &temp.join("preview-aggregate-verified"),
        &plan.products,
        &companions,
        PackageOutputIdentity {
            source_commit: &authenticated_context.source_sha,
            source_ref: &authenticated_context.source_ref,
            version: &authenticated_context.version,
        },
    )?;
    let mut actual = verified
        .artifacts
        .iter()
        .map(|row| handoff::AssetProof {
            name: row.name.clone(),
            sha256: row.sha256.clone(),
            size: row.size,
        })
        .collect::<Vec<_>>();
    actual.sort_by(|a, b| a.name.cmp(&b.name));
    if actual != proof.assets {
        return Err(internal("preview_aggregate_verified_inventory"));
    }
    handoff::checkpoint(deadline)
}

fn entries(
    proof: &PreviewAggregateManifest,
    metadata: &[u8],
    manifest: &[u8],
) -> Vec<ExpectedZipEntry> {
    let mut entries = proof
        .assets
        .iter()
        .map(|asset| ExpectedZipEntry {
            name: asset.name.clone(),
            size: Some(asset.size),
            sha256: Some(asset.sha256.clone()),
            max_bytes: asset.size,
            unix_permissions: Some(0o644),
        })
        .collect::<Vec<_>>();
    for (name, bytes) in [
        (handoff::AGGREGATE_MANIFEST, metadata),
        ("package-output.json", manifest),
    ] {
        entries.push(ExpectedZipEntry {
            name: name.to_owned(),
            size: Some(bytes.len() as u64),
            sha256: Some(handoff::sha256(bytes)),
            max_bytes: handoff::MAX_NATIVE_MANIFEST,
            unix_permissions: Some(0o644),
        });
    }
    entries
}

fn copy_full_input(
    downloaded: &Path,
    input: &Path,
    proof: &PreviewAggregateManifest,
    manifest: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for asset in &proof.assets {
        let bytes = handoff::read_verified(&downloaded.join(&asset.name), asset, deadline)?;
        write_exclusive_until(
            &input.join(&asset.name),
            &bytes,
            "preview_signer_input",
            || handoff::checkpoint(deadline),
        )?;
    }
    write_exclusive_until(
        &input.join("package-output.json"),
        manifest,
        "preview_signer_handoff",
        || handoff::checkpoint(deadline),
    )
}

fn runner_temp() -> Result<std::path::PathBuf, OrchestratorError> {
    let temp = std::path::PathBuf::from(context::required_env("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| internal("preview_runner_temp"))?;
    let root = temp.join("velnor");
    crate::exclusive_write::create_dir_no_symlink(&temp, &root)?;
    Ok(root)
}
