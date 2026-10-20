//! Privileged-job admission of authenticated aggregate bytes; executes no source.

use super::{context, handoff, leaf_manifest::PreviewAggregateManifest};
use crate::{
    OrchestratorError,
    exclusive_write::write_exclusive_until,
    internal::internal,
    package_release_artifacts::{PackageOutputIdentity, verify_package_output},
    package_release_publish::{
        acquire_release_aggregate,
        artifact_zip::{self, ExpectedZipEntry, ZipProfile},
    },
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use velnor_actions_mise::CheckDeadline;

/// Admit the exact aggregate service ID before the separate, unchanged signer.
pub const ADMIT_RELEASE_AGGREGATE_OP: &str = "admit-release-aggregate-v1";

/// Authenticate and inspect the full union without running any repository task.
/// # Errors
/// Rejects role, source, remote authority, ZIP, native proof or full artifact drift.
pub fn admit_release_aggregate() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(
        Instant::now(),
        Duration::from_secs(u64::from(velnor_actions_contract::JobTimeout::DEFAULT_MINUTES) * 60),
    )
    .map_err(|error| internal(&error.to_string()))?;
    deadline
        .scope(|| admit_until(deadline))
        .map_err(|error| internal(&error.to_string()))?
}

fn admit_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let temp = runner_temp()?;
    let (policy, plan) = crate::release_policy_admission::admitted_policy_plan(&temp, deadline)?;
    if context::required_env("GITHUB_JOB")? != plan.channel.signer_job_id() {
        return Err(internal("preview_signer_role"));
    }
    let policy_sha256 = handoff::sha256(
        &policy
            .canonical_bytes()
            .map_err(|_| internal("release_policy_digest"))?,
    );
    let authenticated_context = context::workflow_context(&plan, deadline)?;
    let id = context::positive_env("VELNOR_RELEASE_AGGREGATE_ARTIFACT_ID")?;
    let digest = context::required_env("VELNOR_RELEASE_AGGREGATE_ARTIFACT_DIGEST")?;
    let archive_dir = handoff::fresh_directory(&temp, "preview-aggregate-download")?;
    let mut artifact = acquire_release_aggregate(
        &plan,
        &authenticated_context,
        id,
        &digest,
        &archive_dir,
        deadline,
    )?;
    let metadata = artifact_zip::read_manifest_file(
        artifact.archive.reader()?,
        ZipProfile::PreviewAggregate,
        handoff::AGGREGATE_MANIFEST,
        handoff::MAX_NATIVE_MANIFEST,
        deadline,
    )?;
    let proof =
        PreviewAggregateManifest::parse(&metadata, &policy_sha256, &plan, &authenticated_context)?;
    let source_manifest = artifact_zip::read_manifest_file(
        artifact.archive.reader()?,
        ZipProfile::PreviewAggregate,
        "package-output.json",
        crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES,
        deadline,
    )?;
    let downloaded = handoff::fresh_directory(&temp, "preview-aggregate-acquired")?;
    let entries = entries(&proof, &metadata, &source_manifest);
    artifact_zip::extract_file(
        artifact.archive.reader()?,
        ZipProfile::PreviewAggregate,
        &downloaded,
        &entries,
        deadline,
    )?;
    artifact.archive.verify(deadline)?;
    drop(artifact);
    let input = handoff::fresh_directory(&temp, "preview-input")?;
    copy_full_input(&downloaded, &input, &proof, &source_manifest, deadline)?;
    let companions = preview_companions(&plan);
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

fn preview_companions(
    plan: &velnor_actions_contract::PackageReleasePlan,
) -> Vec<velnor_actions_contract::config::RustBinaryReleaseCompanion> {
    plan.companions
        .iter()
        .map(
            |row| velnor_actions_contract::config::RustBinaryReleaseCompanion {
                id: row.id.clone(),
                path: row.path.clone(),
                asset_name: row.asset_name.clone(),
            },
        )
        .collect()
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
        handoff::copy_verified(
            &downloaded.join(&asset.name),
            &input.join(&asset.name),
            asset,
            deadline,
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
