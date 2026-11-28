// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::github::{Admin, remove_runner_named};
use crate::launch_blocking::ListenFault;
use crate::rust_runner::{ReservationId, RustRunnerLauncher};

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
    inputs.launcher.clean_job(inputs.reservation)?;
    remove_runner_named(&inputs.admin, inputs.scale_set_id, &inputs.runner_name)?;
    Ok(CleanupProof {
        reservation: inputs.reservation,
    })
}
