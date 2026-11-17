// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

use bollard::Docker;
use tokio::time::Instant;

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, IntentState, Journal, PhysicalRunnerJitState, ReservationState,
};
use crate::launch::{ImageAuthority, SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::storage_initialization::recover_absent;

use super::unbound_retirement::retire_unbound_pair_before;
use super::{recover_pre_jit_reservation, retire_unbound_pair};

pub(crate) const INITIALIZER_RECOVERY_BUDGET: Duration = Duration::from_secs(30);

pub(crate) struct UncertainRecovery<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) scope: &'a ConnectionJournal,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) image_authority: &'a ImageAuthority,
    pub(crate) pass: usize,
}

pub(crate) enum UncertainRecoveryWork {
    Initializer(InitializerRecovery),
    AssignedPair(AssignedPairRecovery),
    UnboundPair(UnboundPairRecovery),
}

impl UncertainRecoveryWork {
    pub(crate) fn reservation_id(&self) -> i64 {
        match self {
            Self::Initializer(work) => work.reservation_id,
            Self::AssignedPair(work) => work.reservation_id,
            Self::UnboundPair(work) => work.reservation_id,
        }
    }
}

pub(crate) async fn uncertain_recovery_work(
    recovery: &UncertainRecovery<'_>,
) -> Result<Vec<UncertainRecoveryWork>, HostError> {
    let mut work = BTreeMap::new();
    for reservation in recovery.scope.physical_runner_reservations().await? {
        if reservation.reservation.state != ReservationState::Uncertain
            || reservation.intent_state != Some(IntentState::Uncertain)
            || reservation.jit_state != PhysicalRunnerJitState::Absent
        {
            continue;
        }
        let id = reservation.reservation_id;
        let candidate = if recovery
            .scope
            .storage_initializer_abort_candidate(id)
            .await?
            .is_some()
        {
            UncertainRecoveryWork::Initializer(InitializerRecovery { reservation_id: id })
        } else if reservation.ownership.assignment().is_some() {
            UncertainRecoveryWork::AssignedPair(AssignedPairRecovery {
                reservation_id: id,
                acquired_request: reservation.acquired_request.is_some(),
            })
        } else {
            continue;
        };
        work.insert(candidate.reservation_id(), candidate);
    }
    for reservation_id in recovery.scope.uncertain_unbound_pairs().await? {
        work.entry(reservation_id).or_insert_with(|| {
            UncertainRecoveryWork::UnboundPair(UnboundPairRecovery { reservation_id })
        });
    }
    let mut work: Vec<_> = work.into_values().collect();
    let first = recovery.pass % work.len().max(1);
    work.rotate_left(first);
    Ok(work)
}

pub(crate) struct InitializerRecovery {
    reservation_id: i64,
}

impl InitializerRecovery {
    pub(crate) async fn recover_before(
        self,
        recovery: &UncertainRecovery<'_>,
        initializer_deadline: Instant,
        connection_deadline: Instant,
    ) {
        if let Err(error) = recover_absent(
            recovery.docker,
            recovery.scope,
            self.reservation_id,
            initializer_deadline,
        )
        .await
        {
            report(
                recovery.scope,
                self.reservation_id,
                "an interrupted storage initializer",
                error,
            );
        }
        UnboundPairRecovery {
            reservation_id: self.reservation_id,
        }
        .recover_before(recovery, connection_deadline)
        .await;
    }
}

pub(crate) struct AssignedPairRecovery {
    reservation_id: i64,
    acquired_request: bool,
}

impl AssignedPairRecovery {
    pub(crate) async fn recover(self, recovery: &UncertainRecovery<'_>) {
        if self.acquired_request
            && let Err(error) = Box::pin(recover_pre_jit_reservation(
                recovery.docker,
                recovery.journal,
                recovery.scope,
                recovery.engine,
                recovery.images,
                self.reservation_id,
                recovery.image_authority,
            ))
            .await
        {
            report(
                recovery.scope,
                self.reservation_id,
                "an uncertain pre-JIT reservation",
                error,
            );
        }
        if let Err(error) = Box::pin(retire_unbound_pair(
            recovery.docker,
            recovery.scope,
            recovery.engine,
            self.reservation_id,
        ))
        .await
        {
            report(
                recovery.scope,
                self.reservation_id,
                "an unbound pair",
                error,
            );
        }
    }
}

pub(crate) struct UnboundPairRecovery {
    reservation_id: i64,
}

impl UnboundPairRecovery {
    pub(crate) async fn recover_before(
        self,
        recovery: &UncertainRecovery<'_>,
        connection_deadline: Instant,
    ) {
        if Instant::now() >= connection_deadline {
            return;
        }
        retire_unbound_pair_before(
            connection_deadline,
            recovery.docker,
            recovery.scope,
            recovery.engine,
            self.reservation_id,
        )
        .await;
    }
}

fn report(scope: &ConnectionJournal, reservation_id: i64, work: &str, error: HostError) {
    eprintln!(
        "The controller keeps {work} charged for recovery. connection={} reservation={reservation_id} reason={error}",
        scope.connection_id().as_str()
    );
}
