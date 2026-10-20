use std::future::Future;
use std::time::{Duration, Instant};

use bollard::{Docker, errors::Error as DockerError};

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{ConnectionJournal, PreJitCleanupClaim, PreJitCleanupInventory},
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

use super::PreJitCleanupPhase;

#[path = "initializer.rs"]
mod initializer;
#[path = "bootstrap/phase.rs"]
mod phase;

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn remove_pair_if_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
) -> Result<(), HostError> {
    let Some(phase) = journal.pre_jit_cleanup_phase(claim).await? else {
        return Err(HostError::Journal);
    };
    if phase == PreJitCleanupPhase::Claimed {
        phase::remove_bootstrap_phase(docker, journal, engine, images, claim, inventory).await?;
    }
    let Some(phase) = journal.pre_jit_cleanup_phase(claim).await? else {
        return Err(HostError::Journal);
    };
    if phase == PreJitCleanupPhase::BootstrapAbsent {
        initializer::remove_if_claimed(docker, journal, engine, images, claim, inventory).await?;
    }
    let Some(phase) = journal.pre_jit_cleanup_phase(claim).await? else {
        return Err(HostError::Journal);
    };
    if phase == PreJitCleanupPhase::InitializerAbsent {
        super::storage::remove_if_claimed(docker, journal, engine, images, claim, inventory)
            .await?;
    }
    let Some(phase) = journal.pre_jit_cleanup_phase(claim).await? else {
        return Err(HostError::Journal);
    };
    if phase == PreJitCleanupPhase::StorageAbsent {
        super::network::remove_if_claimed(docker, journal, engine, images, claim, inventory)
            .await?;
    }
    Ok(())
}

pub(super) async fn verify_selection(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    deadline: Instant,
) -> Result<SelectionObservation, HostError> {
    if !engine.matches_images(images)
        || engine.engine_id() != claim.engine_id()
        || engine.docker_root_digest() != claim.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }
    let selected = crate::journal::EngineContext {
        engine_id: engine.engine_id().to_owned(),
        endpoint: engine.endpoint().to_owned(),
        platform: engine.platform().to_owned(),
    };
    let info = request(deadline, docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let observed = SelectedEngineContext::capture(&selected, &info)?;
    if !observed.matches_images(images) {
        return Err(HostError::Ownership);
    }
    images.verify_for_engine(docker, engine).await?;
    Ok(SelectionObservation {
        engine_id: observed.engine_id().to_owned(),
        docker_root_digest: observed.docker_root_digest().to_owned(),
        platform: observed.platform().to_owned(),
        daemon_architecture: observed.daemon_architecture().to_owned(),
    })
}

pub(super) async fn inspect_container(
    docker: &Docker,
    name_or_id: &str,
    deadline: Instant,
) -> Result<Option<bollard::models::ContainerInspectResponse>, HostError> {
    match request(deadline, docker.inspect_container(name_or_id, None)).await? {
        Ok(value) => Ok(Some(value)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn request<F: Future>(
    deadline: Instant,
    future: F,
) -> Result<F::Output, HostError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(HostError::Docker)?;
    docker_deadline_after(future, remaining.min(REQUEST_TIMEOUT)).await
}

pub(super) struct SelectionObservation {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) platform: String,
    pub(super) daemon_architecture: String,
}
