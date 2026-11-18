// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{path::Path, sync::Arc};

use bollard::Docker;
use tokio::time::Instant;

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
    reconcile::Reconcile,
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
    pub(super) hold_log: HoldLog,
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

    let legacy = crate::launch::evaluate_gate(journal, docker, current, state).await?;
    eprintln!(
        "legacy_reconcile={}",
        crate::launch::gate_line(&legacy.decision)
    );
    let mut hold_log = HoldLog::default();
    if !admission_after(true, &legacy.decision) && hold_log.on_hold(&legacy.hold_lines) {
        print_hold_lines(&legacy.hold_lines);
    }
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
    let storage_ready = finish_startup(
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
    // The gate is ANDed outside the storage profile: `storage_ready` keeps meaning "storage ok",
    // and a Hold leaves admission closed while every other startup step above still ran.
    let admission_ready = admission_after(storage_ready, &legacy.decision);
    Ok(StartupResources {
        worker_images,
        admission_ready,
        hold_log,
    })
}

/// The single decision point for opening admission, shared by startup and the storage-profile
/// retry: storage must be ready AND the legacy gate must say Advertise. A Hold, and a gate that
/// could not be evaluated at all, never open admission.
pub(super) fn admission_after(storage_ready: bool, gate: &Reconcile) -> bool {
    storage_ready && matches!(gate, Reconcile::Advertise { .. })
}

/// Re-runs the legacy gate for the storage-profile retry. True only when the gate says Advertise;
/// an evaluation error is closed, never open. Logs each hold (rate limited) and the heal.
///
/// The retry only runs while admission is closed, so an open host never re-checks the gate. That
/// is acceptable because nothing but tests writes legacy rows (`begin_launch` has no live caller).
pub(super) async fn legacy_admission(
    journal: &Journal,
    docker: &Docker,
    config: &HostConfig,
    state: &Path,
    log: &mut HoldLog,
) -> bool {
    let report = match crate::launch::evaluate_gate(journal, docker, config, state).await {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "legacy reconcile gate could not be evaluated; admission stays closed: {error:?}"
            );
            return false;
        }
    };
    if admission_after(true, &report.decision) {
        if log.on_open() {
            eprintln!("legacy hold cleared; admission reopened");
        }
        return true;
    }
    eprintln!(
        "admission closed by legacy reconciliation ({})",
        crate::launch::gate_line(&report.decision)
    );
    if log.on_hold(&report.hold_lines) {
        print_hold_lines(&report.hold_lines);
    }
    false
}

fn print_hold_lines(lines: &[String]) {
    for line in lines {
        eprintln!("{line}");
    }
}

const HOLD_REPEAT_TICKS: u32 = 10;

/// Rate limit for `legacy_hold` lines: print when the set of lines changes, and again every
/// `HOLD_REPEAT_TICKS` unchanged ticks (about 5 minutes at the 30 s retry cadence).
#[derive(Debug, Default)]
pub(super) struct HoldLog {
    last: Option<Vec<String>>,
    unchanged_ticks: u32,
}

impl HoldLog {
    pub(super) fn on_hold(&mut self, lines: &[String]) -> bool {
        let unchanged = self.last.as_deref() == Some(lines);
        if unchanged && self.unchanged_ticks + 1 < HOLD_REPEAT_TICKS {
            self.unchanged_ticks += 1;
            return false;
        }
        self.last = Some(lines.to_vec());
        self.unchanged_ticks = 0;
        true
    }

    fn on_open(&mut self) -> bool {
        self.unchanged_ticks = 0;
        self.last.take().is_some()
    }
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

#[cfg(test)]
#[path = "startup/tests.rs"]
mod tests;
