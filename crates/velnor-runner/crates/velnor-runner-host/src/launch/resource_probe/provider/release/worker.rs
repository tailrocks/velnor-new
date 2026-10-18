use std::collections::HashMap;

use super::gh::{self, Workspace};
use super::metadata;
use super::{
    CHECKSUMS_FILE, DIND_ARCHIVE, MAX_CHECKSUMS_BYTES, PreparedWorkerArtifact,
    PreparedWorkerReleases, ProviderFailure, RUNNER_ARCHIVE, download_selected_assets, sha256,
    valid_sha, verify_release, verify_worker_asset_attestations,
};

pub(in crate::launch::resource_probe::provider) async fn prepare_worker_releases(
    pat: &str,
    source_revision: &str,
) -> Result<PreparedWorkerReleases, ProviderFailure> {
    if !valid_sha(source_revision) {
        return Err(ProviderFailure::TrustedSourcePinInvalid);
    }
    let tag = format!("runner-{source_revision}");
    let workspace = Workspace::create()?;
    gh::assert_version(&workspace, pat).await?;
    let assets = verify_release(&workspace, pat, &tag, source_revision).await?;
    download_selected_assets(
        &workspace,
        pat,
        &tag,
        &assets,
        &[RUNNER_ARCHIVE, DIND_ARCHIVE, CHECKSUMS_FILE],
    )
    .await?;
    let (runner, dind) = prepare_worker_artifacts(&workspace, &assets, source_revision)?;
    verify_worker_asset_attestations(
        &workspace,
        pat,
        &tag,
        source_revision,
        &assets,
        &runner,
        &dind,
    )
    .await?;
    Ok(PreparedWorkerReleases {
        _workspace: workspace,
        runner,
        dind,
    })
}

fn prepare_worker_artifacts(
    workspace: &Workspace,
    assets: &metadata::HashMapAssets,
    source_revision: &str,
) -> Result<(PreparedWorkerArtifact, PreparedWorkerArtifact), ProviderFailure> {
    metadata::ensure_exact_downloads(
        workspace.assets(),
        &[RUNNER_ARCHIVE, DIND_ARCHIVE, CHECKSUMS_FILE],
    )?;
    for name in [RUNNER_ARCHIVE, DIND_ARCHIVE] {
        if assets.get(name).is_none_or(|asset| {
            asset.size > super::super::archive::worker::MAX_WORKER_ARCHIVE_BYTES
        }) {
            return Err(ProviderFailure::ReleaseAssets);
        }
    }
    let checksums = metadata::read_bounded(
        &workspace.assets().join(CHECKSUMS_FILE),
        MAX_CHECKSUMS_BYTES,
    )?;
    let checksums = metadata::parse_checksums(&checksums)?;
    let runner = prepare_worker_artifact(
        workspace,
        assets,
        &checksums,
        source_revision,
        super::super::archive::worker::WorkerImage::Runner,
    )?;
    let dind = prepare_worker_artifact(
        workspace,
        assets,
        &checksums,
        source_revision,
        super::super::archive::worker::WorkerImage::Dind,
    )?;
    let checksums_path = workspace.assets().join(CHECKSUMS_FILE);
    let checksum_digest = sha256(&metadata::read_bounded(
        &checksums_path,
        MAX_CHECKSUMS_BYTES,
    )?);
    let hashes = HashMap::from([
        (RUNNER_ARCHIVE, runner.original.archive_sha256.clone()),
        (DIND_ARCHIVE, dind.original.archive_sha256.clone()),
        (CHECKSUMS_FILE, checksum_digest),
    ]);
    metadata::verify_asset_digests(assets, &checksums, &hashes)?;
    Ok((runner, dind))
}

fn prepare_worker_artifact(
    workspace: &Workspace,
    assets: &metadata::HashMapAssets,
    checksums: &metadata::HashMapAssets,
    source_revision: &str,
    image: super::super::archive::worker::WorkerImage,
) -> Result<PreparedWorkerArtifact, ProviderFailure> {
    let name = image.archive_name();
    let expected_digest = assets
        .get(name)
        .and_then(|asset| asset.digest.strip_prefix("sha256:"))
        .ok_or(ProviderFailure::AssetChecksum)?;
    let checksum = checksums
        .get(name)
        .and_then(|asset| asset.digest.strip_prefix("sha256:"))
        .ok_or(ProviderFailure::AssetChecksum)?;
    if checksum != expected_digest {
        return Err(ProviderFailure::AssetChecksum);
    }
    let archive_path = workspace.assets().join(name);
    let file_metadata =
        std::fs::symlink_metadata(&archive_path).map_err(|_| ProviderFailure::ReleaseAssets)?;
    if !file_metadata.file_type().is_file()
        || file_metadata.len() != assets.get(name).ok_or(ProviderFailure::ReleaseAssets)?.size
    {
        return Err(ProviderFailure::ReleaseAssets);
    }
    let original = super::super::archive::worker::verify(
        &archive_path,
        image,
        source_revision,
        expected_digest,
        &image.release_tag(),
    )?;
    let output_path = workspace
        .assets()
        .join(format!("{}-load.tar", image.image_name()));
    let load = super::super::archive::worker::derive_load_archive(
        &archive_path,
        &output_path,
        image,
        source_revision,
        &original,
    )?;
    Ok(PreparedWorkerArtifact {
        image,
        original,
        load,
    })
}
