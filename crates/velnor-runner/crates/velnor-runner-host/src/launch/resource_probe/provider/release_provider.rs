use super::{
    Docker, HostError, ImageImportState, ImageInspect, ImageProvider, Journal, PREPARE_TIMEOUT,
    PreparedReleaseCacheKey, ProviderFailure, ReleaseImageProvider, SelectedDaemon, SystemInfo,
    VerifiedCandidate, WorkerReleaseMaterial, candidate, daemon_platform_matches,
    image_import_request, inspect_image, release, selected_daemon, verified_import_inspection,
    verify_daemon_image, verify_selected_daemon,
};

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, OnceCell};

impl ReleaseImageProvider<'_> {
    async fn verified_release(&self) -> Result<Arc<release::PreparedRelease>, ProviderFailure> {
        let cells = PREPARED_RELEASES.get_or_init(|| Mutex::new(HashMap::new()));
        let cache_key = self
            .cache_key
            .as_ref()
            .ok_or(ProviderFailure::NativeAdmission)?;
        let cell = {
            let mut cells = cells.lock().await;
            Arc::clone(
                cells
                    .entry(cache_key.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        let prepared = cell
            .get_or_try_init(|| async {
                let native = self
                    .native
                    .as_ref()
                    .ok_or(ProviderFailure::NativeAdmission)?;
                release::prepare_native(native, &self.source_revision)
                    .await
                    .map(Arc::new)
            })
            .await?;
        Ok(Arc::clone(prepared))
    }

    async fn candidate_from_release<F>(
        &self,
        docker: &Docker,
        journal: &Journal,
        selected: &SelectedDaemon,
        release: F,
    ) -> Result<VerifiedCandidate, ProviderFailure>
    where
        F: Future<Output = Result<Arc<release::PreparedRelease>, ProviderFailure>>,
    {
        let release = release.await?;
        verify_selected_daemon(docker, selected).await?;
        let image = self
            .inspect_or_load(docker, journal, selected, &release)
            .await?;
        candidate(
            &selected.engine_id,
            &selected.docker_root_digest,
            &selected.architecture,
            image,
            &release,
        )
    }

    pub(super) async fn verified_candidate_with_release<F>(
        &self,
        docker: &Docker,
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
        release: F,
    ) -> Result<Option<VerifiedCandidate>, ProviderFailure>
    where
        F: Future<Output = Result<Arc<release::PreparedRelease>, ProviderFailure>>,
    {
        if !daemon_platform_matches(info) {
            return Ok(None);
        }
        let selected = selected_daemon(info, engine_id)?;
        let candidate = tokio::time::timeout(
            PREPARE_TIMEOUT,
            self.candidate_from_release(docker, journal, &selected, release),
        )
        .await
        .map_err(|_| ProviderFailure::Timeout)??;
        Ok(Some(candidate))
    }

    pub(super) async fn inspect_or_load(
        &self,
        docker: &Docker,
        journal: &Journal,
        selected: &SelectedDaemon,
        release: &release::PreparedRelease,
    ) -> Result<ImageInspect, ProviderFailure> {
        let request = image_import_request(selected, release)?;
        let record = journal
            .prepare_image_import(request.clone())
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;

        match record.state {
            ImageImportState::Prepared | ImageImportState::Completed => {}
            ImageImportState::Requested | ImageImportState::Uncertain => {
                // The prior /images/load effect may still be in flight. Inspection
                // alone cannot prove its response stream drained or fence its owner.
                return Err(ProviderFailure::ImageLoad);
            }
            ImageImportState::Aborted => return Err(ProviderFailure::ImageImportJournal),
        }

        for reference in [
            release.image_reference.as_str(),
            release.config_digest.as_str(),
            release.manifest_digest.as_str(),
        ] {
            if let Some(image) = inspect_image(docker, reference).await? {
                verify_daemon_image(&image, release)?;
                let inspection =
                    verified_import_inspection(docker, selected, release, &request, &image).await?;
                if record.state == ImageImportState::Prepared {
                    journal
                        .complete_preloaded_image_import(record.id, &request, &inspection)
                        .await
                        .map_err(|_| ProviderFailure::ImageImportJournal)?;
                } else if record.image_id.as_deref() != Some(inspection.image_id()) {
                    return Err(ProviderFailure::ImageIdentity);
                } else {
                    verify_selected_daemon(docker, selected).await?;
                }
                return Ok(image);
            }
        }

        if record.state != ImageImportState::Prepared {
            return Err(ProviderFailure::ImageLoad);
        }

        verify_selected_daemon(docker, selected).await?;
        let claim = journal
            .claim_image_import_effect(record.id, &request)
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;
        let Some(claim) = claim else {
            return Err(ProviderFailure::ImageLoad);
        };

        let result = self.load_and_inspect(docker, selected, release).await;
        let image = match result {
            Ok(image) => image,
            Err(error) => {
                journal
                    .mark_image_import_uncertain(claim)
                    .await
                    .map_err(|_| ProviderFailure::ImageImportJournal)?;
                return Err(error);
            }
        };
        let inspection =
            match verified_import_inspection(docker, selected, release, &request, &image).await {
                Ok(inspection) => inspection,
                Err(error) => {
                    journal
                        .mark_image_import_uncertain(claim)
                        .await
                        .map_err(|_| ProviderFailure::ImageImportJournal)?;
                    return Err(error);
                }
            };
        journal
            .complete_claimed_image_import(claim, &inspection)
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;
        Ok(image)
    }

    async fn load_and_inspect(
        &self,
        docker: &Docker,
        selected: &SelectedDaemon,
        release: &release::PreparedRelease,
    ) -> Result<ImageInspect, ProviderFailure> {
        // The durable Requested row was committed before this selected-engine check.
        // Any mismatch after the claim remains unresolved and blocks another import.
        verify_selected_daemon(docker, selected).await?;
        if inspect_image(docker, &release.image_reference)
            .await?
            .is_some()
        {
            return Err(ProviderFailure::ImageIdentity);
        }
        verify_selected_daemon(docker, selected).await?;
        let response = docker.import_image(
            bollard::query_parameters::ImportImageOptionsBuilder::default().build(),
            bollard::body_full(release.load_archive.clone().into()),
            None,
        );
        tokio::time::timeout(PREPARE_TIMEOUT, async {
            futures_util::pin_mut!(response);
            while let Some(item) = futures_util::StreamExt::next(&mut response).await {
                item.map_err(|_| ProviderFailure::ImageLoad)?;
            }
            Ok::<(), ProviderFailure>(())
        })
        .await
        .map_err(|_| ProviderFailure::ImageLoad)??;
        verify_selected_daemon(docker, selected).await?;
        let image = inspect_image(docker, &release.image_reference)
            .await?
            .ok_or(ProviderFailure::ImageInspect)?;
        verify_daemon_image(&image, release)?;
        Ok(image)
    }
}

impl ReleaseImageProvider<'_> {
    pub(super) fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub(super) async fn download_worker_material(
        &self,
        directory: &std::path::Path,
    ) -> Result<WorkerReleaseMaterial, ProviderFailure> {
        self.native
            .as_ref()
            .ok_or(ProviderFailure::NativeAdmission)?
            .download_worker_material(directory)
            .await
            .map_err(|_| ProviderFailure::NativeAdmission)
    }
}

impl ImageProvider for ReleaseImageProvider<'_> {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        let result = self
            .verified_candidate_with_release(
                docker,
                journal,
                engine_id,
                info,
                self.verified_release(),
            )
            .await;
        match result {
            Ok(Some(candidate)) => Ok(Some(candidate)),
            Ok(None) => {
                eprintln!("resource_probe=unavailable reason=selected_daemon_platform_mismatch");
                Ok(None)
            }
            Err(error) => {
                eprintln!("resource_probe=unavailable reason={}", error.code());
                Err(HostError::Docker)
            }
        }
    }
}

type PreparedReleaseCell = Arc<OnceCell<Arc<release::PreparedRelease>>>;

static PREPARED_RELEASES: OnceLock<Mutex<HashMap<PreparedReleaseCacheKey, PreparedReleaseCell>>> =
    OnceLock::new();
