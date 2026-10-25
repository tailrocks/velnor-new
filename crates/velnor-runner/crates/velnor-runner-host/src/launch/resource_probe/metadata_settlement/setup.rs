use std::time::{Duration, Instant};

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, StorageProfile},
    launch::SelectedEngineContext,
};

use super::super::lifecycle::{Deadline, bind_info, guest_capacity, info, same_guest};
use super::super::projection::{DockerRoot, VerifiedProbeImage};
use super::super::provider::{ImageProvider, ProbeImagePreparationEvidence};
use super::super::recover;

pub(super) struct SettlementSetup {
    pub(super) profile: StorageProfile,
    pub(super) image: VerifiedProbeImage,
    pub(super) evidence: ProbeImagePreparationEvidence,
    pub(super) engine_id: String,
    pub(super) root: DockerRoot,
    pub(super) architecture: String,
    pub(super) cpu_count: u32,
    pub(super) memory_total: u64,
    pub(super) deadline: Deadline,
}

pub(super) async fn prepare_settlement_setup(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    provider: &impl ImageProvider,
) -> Result<SettlementSetup, HostError> {
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let preparation = Deadline::new();
    let before = info(docker, &preparation).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &preparation).await?;
    verify_selected(selected, &engine_id, &root)?;
    if let Some(active) = preparation.within(journal.active_probe()).await?? {
        Box::pin(recover::active(
            docker,
            journal,
            &before,
            &root,
            &preparation,
            active,
        ))
        .await?;
    }
    let (image, evidence) = super::super::lifecycle::verify_publication_image_until(
        docker,
        journal,
        provider,
        selected,
        Instant::now() + Duration::from_secs(180),
    )
    .await?;
    let deadline = Deadline::new();
    let selected_info = info(docker, &deadline).await?;
    let architecture =
        super::super::provider::daemon_architecture(&selected_info).ok_or(HostError::Identity)?;
    if !same_guest(&selected_info, &engine_id, &root, architecture)?
        || architecture != selected.daemon_architecture()
    {
        return Err(HostError::Identity);
    }
    let (cpu_count, memory_total) = guest_capacity(&selected_info)?;
    Ok(SettlementSetup {
        profile,
        image,
        evidence,
        engine_id,
        root,
        architecture: architecture.to_owned(),
        cpu_count,
        memory_total,
        deadline,
    })
}

fn verify_selected(
    selected: &SelectedEngineContext,
    engine_id: &str,
    root: &DockerRoot,
) -> Result<(), HostError> {
    if selected.engine_id() == engine_id
        && selected.docker_root_digest() == root.digest()
        && selected.platform() == "linux/amd64"
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}
