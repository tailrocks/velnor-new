use std::time::Instant;

use bollard::{
    Docker, errors::Error as DockerError, query_parameters::RemoveContainerOptionsBuilder,
};
use serde::Serialize;

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, PreJitCleanupClaim, PreJitCleanupInventory, PreJitInitializerIdentity,
    },
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

use super::super::{PreJitCleanupPhase, PreJitCleanupPhaseProof};
use super::{CLEANUP_TIMEOUT, inspect_container, request, verify_selection};

pub(super) async fn remove_if_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
) -> Result<(), HostError> {
    let identity = inventory.initializer();
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    verify_identity(identity, inventory, claim, images)?;
    verify_selection(docker, engine, images, claim, deadline).await?;
    let cleanup = InitializerCleanup {
        docker,
        journal,
        engine,
        images,
        claim,
        deadline,
    };
    let Some(inspected) = inspect_container(docker, identity.helper_name(), deadline).await? else {
        return cleanup.record_if_absent(identity).await;
    };
    cleanup.remove_present(identity, inspected).await
}

struct InitializerCleanup<'a> {
    docker: &'a Docker,
    journal: &'a ConnectionJournal,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
    claim: &'a PreJitCleanupClaim,
    deadline: Instant,
}

impl InitializerCleanup<'_> {
    async fn record_if_absent(
        &self,
        identity: &PreJitInitializerIdentity,
    ) -> Result<(), HostError> {
        if let Some(helper_id) = identity.helper_id()
            && inspect_container(self.docker, helper_id, self.deadline)
                .await?
                .is_some()
        {
            return Err(HostError::Ownership);
        }
        record_absence(
            self.docker,
            self.journal,
            self.engine,
            self.images,
            self.claim,
            identity,
            None,
            None,
            "already_absent",
            self.deadline,
        )
        .await
    }

    async fn remove_present(
        &self,
        identity: &PreJitInitializerIdentity,
        inspected: bollard::models::ContainerInspectResponse,
    ) -> Result<(), HostError> {
        let helper_id = inspected
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or(HostError::Ownership)?
            .to_owned();
        if identity.helper_id().is_some_and(|saved| saved != helper_id) {
            return Err(HostError::Ownership);
        }
        verify_selection(
            self.docker,
            self.engine,
            self.images,
            self.claim,
            self.deadline,
        )
        .await?;
        let expected_env = self.expected_image_env(identity).await?;
        if !crate::worker::storage_initialization::pre_jit_container_matches(
            &inspected,
            identity,
            &expected_env,
        ) {
            return Err(HostError::Ownership);
        }
        let stopped = self
            .stop_if_running(identity, &helper_id, inspected, &expected_env)
            .await?;
        verify_selection(
            self.docker,
            self.engine,
            self.images,
            self.claim,
            self.deadline,
        )
        .await?;
        self.remove_and_record(identity, &helper_id, &stopped).await
    }

    async fn expected_image_env(
        &self,
        identity: &PreJitInitializerIdentity,
    ) -> Result<Vec<String>, HostError> {
        let image = request(
            self.deadline,
            self.docker.inspect_image(identity.image_runtime_id()),
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        if image.id.as_deref() != Some(identity.image_runtime_id())
            || image.os.as_deref() != Some("linux")
            || image.architecture.as_deref() != Some("amd64")
        {
            return Err(HostError::Ownership);
        }
        image
            .config
            .and_then(|config| config.env)
            .ok_or(HostError::Ownership)
    }

    async fn stop_if_running(
        &self,
        identity: &PreJitInitializerIdentity,
        helper_id: &str,
        inspected: bollard::models::ContainerInspectResponse,
        expected_env: &[String],
    ) -> Result<bollard::models::ContainerInspectResponse, HostError> {
        use bollard::models::ContainerStateStatusEnum as Status;
        let state = inspected.state.as_ref().ok_or(HostError::Ownership)?;
        match state.status.ok_or(HostError::Ownership)? {
            Status::RUNNING | Status::PAUSED | Status::RESTARTING | Status::STOPPING => {
                if state.running != Some(true) {
                    return Err(HostError::Ownership);
                }
                verify_selection(
                    self.docker,
                    self.engine,
                    self.images,
                    self.claim,
                    self.deadline,
                )
                .await?;
                let stop = bollard::query_parameters::StopContainerOptionsBuilder::new()
                    .t(1)
                    .build();
                request(
                    self.deadline,
                    self.docker.stop_container(helper_id, Some(stop)),
                )
                .await?
                .map_err(|_| HostError::Docker)?;
                let stopped = inspect_container(self.docker, helper_id, self.deadline)
                    .await?
                    .ok_or(HostError::Ownership)?;
                if stopped.id.as_deref() != Some(helper_id)
                    || stopped.state.as_ref().and_then(|state| state.running) != Some(false)
                    || !matches!(
                        stopped.state.as_ref().and_then(|state| state.status),
                        Some(Status::EXITED | Status::DEAD)
                    )
                    || !crate::worker::storage_initialization::pre_jit_container_matches(
                        &stopped,
                        identity,
                        expected_env,
                    )
                {
                    return Err(HostError::Ownership);
                }
                Ok(stopped)
            }
            Status::EXITED | Status::DEAD if state.running == Some(false) => Ok(inspected),
            _ => Err(HostError::Ownership),
        }
    }

    async fn remove_and_record(
        &self,
        identity: &PreJitInitializerIdentity,
        helper_id: &str,
        stopped: &bollard::models::ContainerInspectResponse,
    ) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(false).build();
        match request(
            self.deadline,
            self.docker.remove_container(helper_id, Some(options)),
        )
        .await?
        {
            Ok(()) => {}
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => return Err(HostError::Ownership),
            Err(_) => return Err(HostError::Docker),
        }
        if inspect_container(self.docker, helper_id, self.deadline)
            .await?
            .is_some()
            || inspect_container(self.docker, identity.helper_name(), self.deadline)
                .await?
                .is_some()
        {
            return Err(HostError::Ownership);
        }
        record_absence(
            self.docker,
            self.journal,
            self.engine,
            self.images,
            self.claim,
            identity,
            Some(helper_id),
            stopped
                .config
                .as_ref()
                .and_then(|config| config.labels.clone()),
            "removed_and_absent",
            self.deadline,
        )
        .await
    }
}

fn verify_identity(
    identity: &PreJitInitializerIdentity,
    inventory: &PreJitCleanupInventory,
    claim: &PreJitCleanupClaim,
    images: &VerifiedWorkerImages,
) -> Result<(), HostError> {
    if identity.operation_id() != claim.operation_id()
        || identity.engine_id() != claim.engine_id()
        || identity.docker_root_digest() != claim.docker_root_digest()
        || identity.image_source_sha() != images.source_sha()
        || !valid_volume(inventory, "runner-home", identity.home_volume(), None)
        || !valid_volume(inventory, "runner-work", identity.work_volume(), None)
        || !valid_volume(
            inventory,
            "engine-metadata",
            identity.metadata_volume(),
            identity.metadata_volume_id(),
        )
        || identity.generation() <= 0
        || identity.cpu_nano() <= 0
        || identity.memory_bytes() <= 0
        || identity.pids_limit() <= 0
        || !matches!(identity.layout_version(), 1 | 2)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn valid_volume(
    inventory: &PreJitCleanupInventory,
    role: &str,
    name: &str,
    id: Option<&str>,
) -> bool {
    let rows = inventory
        .storage_objects()
        .iter()
        .filter(|object| object.role() == role)
        .collect::<Vec<_>>();
    rows.len() == 1
        && rows[0].name() == name
        && id.is_none_or(|expected| rows[0].id() == Some(expected))
        && rows[0].create_state() == "created"
}

async fn record_absence(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    identity: &PreJitInitializerIdentity,
    observed_id: Option<&str>,
    labels: Option<std::collections::HashMap<String, String>>,
    outcome: &'static str,
    deadline: Instant,
) -> Result<(), HostError> {
    let selected = verify_selection(docker, engine, images, claim, deadline).await?;
    let evidence = serde_json::to_vec(&InitializerAbsenceEvidence {
        engine_id: selected.engine_id,
        docker_root_digest: selected.docker_root_digest,
        helper_name: identity.helper_name(),
        helper_id: identity.helper_id(),
        operation_key: identity.operation_key(),
        image_runtime_id: identity.image_runtime_id(),
        image_source_sha: identity.image_source_sha(),
        layout_version: identity.layout_version(),
        home_volume: identity.home_volume(),
        work_volume: identity.work_volume(),
        metadata_volume: identity.metadata_volume(),
        observed_id,
        labels,
        outcome,
    })
    .map_err(|_| HostError::Identity)?;
    let proof = PreJitCleanupPhaseProof::after_observed_phase(
        claim,
        PreJitCleanupPhase::BootstrapAbsent,
        PreJitCleanupPhase::InitializerAbsent,
        &evidence,
    )?;
    journal.record_pre_jit_cleanup_phase(claim, &proof).await
}

#[derive(Serialize)]
struct InitializerAbsenceEvidence<'a> {
    engine_id: String,
    docker_root_digest: String,
    helper_name: &'a str,
    helper_id: Option<&'a str>,
    operation_key: &'a str,
    image_runtime_id: &'a str,
    image_source_sha: &'a str,
    layout_version: i64,
    home_volume: &'a str,
    work_volume: &'a str,
    metadata_volume: &'a str,
    observed_id: Option<&'a str>,
    labels: Option<std::collections::HashMap<String, String>>,
    outcome: &'static str,
}
