use std::{path::Path, sync::Arc};

use bollard::Docker;

use crate::{
    config::HostConfig,
    daemon_lock::{DaemonLock, EngineLineageGuard},
    docker_client::docker_deadline,
    error::HostError,
    journal::Journal,
    launch::{
        ReleaseReadSecret, SelectedEngineContext, SessionRefreshStartupAuthority,
        VerifiedWorkerImages, prepare_worker_images, startup_storage_profile,
    },
    launch_blocking::ListenFault,
};

use super::super::setup;

pub(super) struct StartupInputs<'a> {
    pub(super) current: &'a HostConfig,
    pub(super) daemon_lock: &'a DaemonLock,
    pub(super) state: &'a Path,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) journal: &'a Journal,
    pub(super) lineage: &'a EngineLineageGuard,
    pub(super) release_read_secret: &'a Arc<ReleaseReadSecret>,
}

pub(super) struct StartupResources {
    pub(super) worker_images: VerifiedWorkerImages,
    pub(super) admission_ready: bool,
}

pub(super) async fn prepare_startup(
    inputs: StartupInputs<'_>,
) -> Result<StartupResources, ListenFault> {
    let StartupInputs {
        current,
        daemon_lock,
        state,
        docker,
        selected_engine,
        journal,
        lineage,
        release_read_secret,
    } = inputs;

    let legacy_reconcile = crate::launch::reconcile_gate(journal, docker).await?;
    eprintln!(
        "legacy_reconcile={}",
        crate::launch::gate_line(&legacy_reconcile)
    );
    setup::configure_shared_capacity(journal, current).await?;
    journal.bootstrap_header_authority().await?;
    journal.validate_header_authority().await?;
    let fresh_info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let startup_authority = SessionRefreshStartupAuthority::after_validated_startup(
        daemon_lock,
        state,
        lineage,
        journal,
        selected_engine,
        &fresh_info,
    )
    .await?;
    journal
        .recover_interrupted_host_work(startup_authority)
        .await?;

    let worker_images =
        prepare_worker_images(docker, journal, selected_engine, release_read_secret).await?;
    let admission_ready = prepare_storage_profile(
        current,
        docker,
        journal,
        release_read_secret,
        &worker_images,
    )
    .await?;
    Ok(StartupResources {
        worker_images,
        admission_ready,
    })
}

async fn prepare_storage_profile(
    current: &HostConfig,
    docker: &Docker,
    journal: &Journal,
    release_read_secret: &ReleaseReadSecret,
    worker_images: &VerifiedWorkerImages,
) -> Result<bool, ListenFault> {
    let profile = startup_storage_profile(
        docker,
        journal,
        current.resource_budget()?,
        current.max_jobs(),
        release_read_secret,
        worker_images,
    )
    .await;
    if let Some(profile) = profile {
        match journal.configure_storage_profile(profile).await {
            Ok(()) => Ok(true),
            Err(error) => {
                eprintln!("host controller storage profile persistence held: {error}");
                Ok(false)
            }
        }
    } else {
        eprintln!(
            "host controller storage profile unavailable; positive acquisition remains closed"
        );
        Ok(false)
    }
}
