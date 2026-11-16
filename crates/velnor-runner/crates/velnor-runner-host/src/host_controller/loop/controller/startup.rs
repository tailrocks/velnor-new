// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{path::Path, sync::Arc, time::Instant};

use bollard::Docker;

use crate::{
    config::HostConfig,
    daemon_lock::{DaemonLock, EngineLineageGuard},
    docker_client::docker_deadline,
    error::HostError,
    journal::Journal,
    launch::{
        ImageAuthority, SelectedEngineContext, SessionRefreshStartupAuthority,
        VerifiedWorkerImages, prepare_worker_images, startup_storage_profile,
    },
    launch_blocking::ListenFault,
};

use super::super::setup;
pub(in crate::host_controller::loop_driver) async fn run_controller(
    current: HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
    image_authority: ImageAuthority,
) -> Result<(), ListenFault> {
    let controller =
        super::ControllerLoop::initialize(current, state, daemon_lock, image_authority).await?;
    Box::pin(controller.run()).await
}

pub(super) struct StartupInputs<'a> {
    pub(super) current: &'a HostConfig,
    pub(super) daemon_lock: &'a DaemonLock,
    pub(super) state: &'a Path,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) journal: &'a Journal,
    pub(super) lineage: &'a EngineLineageGuard,
    pub(super) image_authority: &'a Arc<ImageAuthority>,
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
        image_authority,
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
        prepare_worker_images(docker, journal, selected_engine, image_authority).await?;
    let admission_ready = finish_startup(
        FinishInputs {
            current,
            state,
            docker,
            selected_engine,
            journal,
            image_authority,
        },
        &worker_images,
    )
    .await?;
    Ok(StartupResources {
        worker_images,
        admission_ready,
    })
}

pub(in crate::host_controller::loop_driver) struct FinishInputs<'a> {
    pub(in crate::host_controller::loop_driver) current: &'a HostConfig,
    pub(in crate::host_controller::loop_driver) state: &'a Path,
    pub(in crate::host_controller::loop_driver) docker: &'a Docker,
    pub(in crate::host_controller::loop_driver) selected_engine: &'a SelectedEngineContext,
    pub(in crate::host_controller::loop_driver) journal: &'a Journal,
    pub(in crate::host_controller::loop_driver) image_authority: &'a ImageAuthority,
}

pub(in crate::host_controller::loop_driver) async fn finish_startup(
    inputs: FinishInputs<'_>,
    worker_images: &VerifiedWorkerImages,
) -> Result<bool, ListenFault> {
    let FinishInputs {
        current,
        state,
        docker,
        selected_engine,
        journal,
        image_authority,
    } = inputs;
    setup::reconcile_removed_at_startup(&setup::RemovedStartup {
        state,
        config: current,
        journal,
        docker,
        engine: selected_engine,
        images: worker_images,
        image_authority,
        deadline: Instant::now() + setup::REMOVED_STARTUP_BUDGET,
    })
    .await;
    prepare_storage_profile(current, docker, journal, image_authority, worker_images).await
}

async fn prepare_storage_profile(
    current: &HostConfig,
    docker: &Docker,
    journal: &Journal,
    image_authority: &ImageAuthority,
    worker_images: &VerifiedWorkerImages,
) -> Result<bool, ListenFault> {
    let profile = startup_storage_profile(
        docker,
        journal,
        current.resource_budget()?,
        current.max_jobs(),
        image_authority,
        worker_images,
        &current.docker().endpoint,
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
