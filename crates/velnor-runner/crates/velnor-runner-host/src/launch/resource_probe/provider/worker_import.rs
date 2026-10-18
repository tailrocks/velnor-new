//! Durable, bounded loading of release-verified worker images.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bollard::Docker;
use bollard::models::ImageInspect;
use futures_util::StreamExt;

use crate::journal::{
    ImageImportIdentity, ImageImportInspection, ImageImportObservation, ImageImportRequest,
    ImageImportState, Journal,
};

use super::archive::worker::{MAX_WORKER_ARCHIVE_BYTES, ParsedWorkerArchive, WorkerImage};
use super::identity::{
    SelectedDaemon, image_observation, inspect_image, selected_daemon_info, verify_selected_daemon,
};
use super::release::PreparedWorkerArtifact;
use super::{ProviderFailure, worker_stream};

const IMPORT_TIMEOUT: Duration = Duration::from_mins(30);

pub(super) async fn import_worker_artifact(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedDaemon,
    source_sha: &str,
    artifact: &PreparedWorkerArtifact,
) -> Result<VerifiedWorkerImageIdentityProof, ProviderFailure> {
    import_worker_artifact_with_timeout(
        docker,
        journal,
        selected,
        source_sha,
        artifact,
        IMPORT_TIMEOUT,
    )
    .await
}

async fn import_worker_artifact_with_timeout(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedDaemon,
    source_sha: &str,
    artifact: &PreparedWorkerArtifact,
    import_timeout: Duration,
) -> Result<VerifiedWorkerImageIdentityProof, ProviderFailure> {
    let request = import_request(selected, source_sha, artifact)?;
    let record = journal
        .prepare_image_import(request.clone())
        .await
        .map_err(|_| ProviderFailure::ImageImportJournal)?;
    ensure_import_state(record.state)?;

    if let Some(image) = inspect_image(docker, artifact.load.repo_tag.as_str()).await? {
        let verified =
            verify_worker_inspection(docker, selected, &request, artifact, &image).await?;
        match record.state {
            ImageImportState::Prepared => {
                journal
                    .complete_preloaded_image_import(record.id, &request, &verified.receipt)
                    .await
                    .map_err(|_| ProviderFailure::ImageImportJournal)?;
            }
            ImageImportState::Completed => {
                if record.image_id.as_deref() != Some(verified.image_id.as_str())
                    || record.receipt.as_ref() != Some(&verified.observation)
                {
                    return Err(ProviderFailure::ImageIdentity);
                }
            }
            ImageImportState::Requested
            | ImageImportState::Uncertain
            | ImageImportState::Aborted => {
                return Err(ProviderFailure::ImageLoad);
            }
        }
        return Ok(image_proof(
            selected,
            source_sha,
            artifact.image,
            &artifact.original,
            verified.image_id,
        ));
    }

    if record.state != ImageImportState::Prepared {
        return Err(ProviderFailure::ImageLoad);
    }
    let stream = worker_stream::open_verified_archive(
        &artifact.load.path,
        &artifact.load.sha256,
        MAX_WORKER_ARCHIVE_BYTES,
    )
    .await?;
    verify_selected_daemon(docker, selected).await?;
    let claim = journal
        .claim_image_import_effect(record.id, &request)
        .await
        .map_err(|_| ProviderFailure::ImageImportJournal)?
        .ok_or(ProviderFailure::ImageLoad)?;

    match import_claimed(docker, selected, &request, artifact, stream, import_timeout).await {
        Ok(verified) => {
            let image_id = verified.image_id.clone();
            journal
                .complete_claimed_image_import(claim, &verified.receipt)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?;
            Ok(image_proof(
                selected,
                source_sha,
                artifact.image,
                &artifact.original,
                image_id,
            ))
        }
        Err(error) => {
            journal
                .mark_image_import_uncertain(claim)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?;
            Err(error)
        }
    }
}

fn import_request(
    selected: &SelectedDaemon,
    source_sha: &str,
    artifact: &PreparedWorkerArtifact,
) -> Result<ImageImportRequest, ProviderFailure> {
    let original = &artifact.original;
    let load = &artifact.load;
    ImageImportRequest::new(ImageImportIdentity {
        engine_id: selected.engine_id.clone(),
        docker_root_digest: selected.docker_root_digest.clone(),
        daemon_architecture: selected.architecture.clone(),
        source_sha: source_sha.to_owned(),
        archive_sha256: original.archive_sha256.clone(),
        archive_index_digest: original.archive_index_digest.clone(),
        platform: "linux/amd64".to_owned(),
        manifest_digest: original.manifest_digest.clone(),
        manifest_size: original.manifest_size,
        config_digest: original.config_digest.clone(),
        image_index_digest: Some(original.image_index_digest.clone()),
        image_index_size: Some(original.image_index_size),
        load_archive_sha256: Some(load.sha256.clone()),
        load_archive_index_digest: Some(load.archive_index_digest.clone()),
        image_reference: load.repo_tag.clone(),
    })
    .map_err(|_| ProviderFailure::ImageImportJournal)
}

fn ensure_import_state(state: ImageImportState) -> Result<(), ProviderFailure> {
    match state {
        ImageImportState::Prepared | ImageImportState::Completed => Ok(()),
        ImageImportState::Requested | ImageImportState::Uncertain => {
            Err(ProviderFailure::ImageLoad)
        }
        ImageImportState::Aborted => Err(ProviderFailure::ImageImportJournal),
    }
}

async fn import_claimed(
    docker: &Docker,
    selected: &SelectedDaemon,
    request: &ImageImportRequest,
    artifact: &PreparedWorkerArtifact,
    stream: worker_stream::VerifiedArchiveStream,
    import_timeout: Duration,
) -> Result<VerifiedImportInspection, ProviderFailure> {
    verify_selected_daemon(docker, selected).await?;
    if inspect_image(docker, &artifact.load.repo_tag)
        .await?
        .is_some()
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    verify_selected_daemon(docker, selected).await?;

    let fully_read = stream.completion_token();
    let response = docker.import_image_stream(
        bollard::query_parameters::ImportImageOptionsBuilder::default().build(),
        stream.stream,
        None,
    );
    tokio::time::timeout(
        import_timeout,
        consume_import_response(response, fully_read),
    )
    .await
    .map_err(|_| ProviderFailure::ImageLoad)??;

    verify_selected_daemon(docker, selected).await?;
    let image = inspect_image(docker, &artifact.load.repo_tag)
        .await?
        .ok_or(ProviderFailure::ImageInspect)?;
    verify_worker_inspection(docker, selected, request, artifact, &image).await
}

async fn consume_import_response<S>(
    response: S,
    fully_read: Arc<AtomicBool>,
) -> Result<(), ProviderFailure>
where
    S: futures_util::Stream<Item = Result<bollard::models::BuildInfo, bollard::errors::Error>>,
{
    futures_util::pin_mut!(response);
    while let Some(item) = response.next().await {
        let status = item.map_err(|_| ProviderFailure::ImageLoad)?;
        if status.error_detail.is_some() {
            return Err(ProviderFailure::ImageLoad);
        }
    }
    if fully_read.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(ProviderFailure::ImageLoad)
    }
}

async fn verify_worker_inspection(
    docker: &Docker,
    selected: &SelectedDaemon,
    request: &ImageImportRequest,
    artifact: &PreparedWorkerArtifact,
    image: &ImageInspect,
) -> Result<VerifiedImportInspection, ProviderFailure> {
    verify_worker_image_config(image, &artifact.original)?;
    let observation = image_observation(image)?;
    let observed = selected_daemon_info(docker, selected).await?;
    let receipt = ImageImportInspection::verified(
        request,
        &observed.engine_id,
        &observed.docker_root_digest,
        &observed.architecture,
        observation.clone(),
    )
    .map_err(|_| ProviderFailure::ImageIdentity)?;
    Ok(VerifiedImportInspection {
        image_id: receipt.image_id().to_owned(),
        receipt,
        observation,
    })
}

fn verify_worker_image_config(
    image: &ImageInspect,
    original: &ParsedWorkerArchive,
) -> Result<(), ProviderFailure> {
    if image.os.as_deref() != Some("linux") || image.architecture.as_deref() != Some("amd64") {
        return Err(ProviderFailure::ImageIdentity);
    }
    let config = image.config.as_ref().ok_or(ProviderFailure::ImageConfig)?;
    let actual = serde_json::to_value(config).map_err(|_| ProviderFailure::ImageConfig)?;
    let expected =
        serde_json::to_value(&original.config).map_err(|_| ProviderFailure::ImageConfig)?;
    let root_fs = image.root_fs.as_ref().ok_or(ProviderFailure::ImageConfig)?;
    if actual != expected
        || root_fs.typ != "layers"
        || root_fs.layers.as_ref() != Some(&original.diff_ids)
    {
        return Err(ProviderFailure::ImageConfig);
    }
    Ok(())
}

fn image_proof(
    selected: &SelectedDaemon,
    source_sha: &str,
    image: WorkerImage,
    archive: &ParsedWorkerArchive,
    runtime_id: String,
) -> VerifiedWorkerImageIdentityProof {
    VerifiedWorkerImageIdentityProof {
        engine_id: selected.engine_id.clone(),
        docker_root_digest: selected.docker_root_digest.clone(),
        daemon_architecture: selected.architecture.clone(),
        source_sha: source_sha.to_owned(),
        image,
        runtime_id,
        archive_sha256: archive.archive_sha256.clone(),
        archive_index_digest: archive.archive_index_digest.clone(),
        image_index_digest: archive.image_index_digest.clone(),
        image_index_size: archive.image_index_size,
        manifest_digest: archive.manifest_digest.clone(),
        manifest_size: archive.manifest_size,
        config_digest: archive.config_digest.clone(),
    }
}

struct VerifiedImportInspection {
    image_id: String,
    receipt: ImageImportInspection,
    observation: ImageImportObservation,
}

/// One worker image identity after the release archive and selected store pass verification.
pub(in crate::launch) struct VerifiedWorkerImageIdentityProof {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    image: WorkerImage,
    runtime_id: String,
    archive_sha256: String,
    archive_index_digest: String,
    image_index_digest: String,
    image_index_size: u64,
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
}

impl VerifiedWorkerImageIdentityProof {
    #[cfg(test)]
    pub(super) fn test_fixture(
        selected: &SelectedDaemon,
        source_sha: &str,
        image: WorkerImage,
        nibble: u8,
    ) -> Self {
        let image_index_digest = fixture_digest(nibble);
        Self {
            engine_id: selected.engine_id.clone(),
            docker_root_digest: selected.docker_root_digest.clone(),
            daemon_architecture: selected.architecture.clone(),
            source_sha: source_sha.to_owned(),
            image,
            runtime_id: image_index_digest.clone(),
            archive_sha256: fixture_hex(nibble),
            archive_index_digest: fixture_hex(nibble + 1),
            image_index_digest,
            image_index_size: 856,
            manifest_digest: fixture_digest(nibble + 2),
            manifest_size: 4_472,
            config_digest: fixture_digest(nibble + 3),
        }
    }

    pub(in crate::launch) fn is_runner(&self) -> bool {
        self.image == WorkerImage::Runner
    }

    pub(in crate::launch) fn is_dind(&self) -> bool {
        self.image == WorkerImage::Dind
    }

    pub(in crate::launch) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(in crate::launch) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(in crate::launch) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }

    pub(in crate::launch) fn source_sha(&self) -> &str {
        &self.source_sha
    }

    pub(in crate::launch) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(in crate::launch) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub(in crate::launch) fn archive_index_digest(&self) -> &str {
        &self.archive_index_digest
    }

    pub(in crate::launch) fn image_index_digest(&self) -> &str {
        &self.image_index_digest
    }

    pub(in crate::launch) fn image_index_size(&self) -> u64 {
        self.image_index_size
    }

    pub(in crate::launch) fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub(in crate::launch) fn manifest_size(&self) -> u64 {
        self.manifest_size
    }

    pub(in crate::launch) fn config_digest(&self) -> &str {
        &self.config_digest
    }
}

#[cfg(test)]
fn fixture_digest(nibble: u8) -> String {
    format!("sha256:{}", fixture_hex(nibble))
}

#[cfg(test)]
fn fixture_hex(nibble: u8) -> String {
    format!("{nibble:x}").repeat(64)
}

#[cfg(test)]
mod tests;
