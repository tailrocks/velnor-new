// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::github::{Admin, remove_runner_named};
use crate::error::{HostError, RustRunnerFailure};
use crate::launch_blocking::ListenFault;
use crate::rust_runner::store::Store;
use crate::rust_runner::{JobPaths, ReservationId, RustRunnerLauncher};

#[derive(Debug)]
pub(super) struct CleanupProof {
    reservation: ReservationId,
}

impl CleanupProof {
    pub(super) const fn into_reservation(self) -> ReservationId {
        self.reservation
    }
}

pub(super) struct ProofInputs {
    pub(super) launcher: RustRunnerLauncher,
    pub(super) admin: Admin,
    pub(super) scale_set_id: i64,
    pub(super) runner_name: String,
    pub(super) reservation: ReservationId,
}

pub(super) fn prove(inputs: &ProofInputs) -> Result<CleanupProof, ListenFault> {
    let paths = JobPaths::of(inputs.reservation);
    end_processes(&inputs.launcher, paths.guest_work_dir())?;
    remove_files(&inputs.launcher, &paths)?;
    remove_runner_named(&inputs.admin, inputs.scale_set_id, &inputs.runner_name)?;
    Ok(CleanupProof {
        reservation: inputs.reservation,
    })
}

fn end_processes(launcher: &RustRunnerLauncher, guest_work_dir: &str) -> Result<(), ListenFault> {
    let survivors = naming(launcher, guest_work_dir)?;
    if survivors.is_empty() {
        return Ok(());
    }
    launcher.kill_processes(&survivors)?;
    if naming(launcher, guest_work_dir)?.is_empty() {
        Ok(())
    } else {
        Err(HostError::RustRunner(RustRunnerFailure::ProcessesSurvive).into())
    }
}

fn naming(launcher: &RustRunnerLauncher, guest_work_dir: &str) -> Result<Vec<u32>, HostError> {
    Ok(launcher
        .list_processes()?
        .iter()
        .filter(|process| process.names_work_dir(guest_work_dir))
        .map(|process| process.pid)
        .collect())
}

fn remove_files(launcher: &RustRunnerLauncher, paths: &JobPaths) -> Result<(), ListenFault> {
    let left = || HostError::RustRunner(RustRunnerFailure::JobFilesLeft);
    let store = Store::open(launcher.host_store()).map_err(|_| left())?;
    store.remove_jit(paths.jit_name()).map_err(|_| left())?;
    launcher.remove_job_dirs(paths)?;
    if store.jit_remains(paths.jit_name()).map_err(|_| left())? {
        return Err(left().into());
    }
    Ok(())
}
