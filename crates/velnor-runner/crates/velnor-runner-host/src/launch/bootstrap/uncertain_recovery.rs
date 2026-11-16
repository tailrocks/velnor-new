// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::time::Duration;

use bollard::Docker;
use tokio::time::Instant;

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, IntentState, Journal, PhysicalRunnerJitState, PhysicalRunnerReservation,
    ReservationState,
};
use crate::launch::{ImageAuthority, SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::storage_initialization::recover_absent;

use super::{recover_pre_jit_reservation, retire_unbound_pair};

const INITIALIZER_RECOVERY_BUDGET: Duration = Duration::from_secs(30);

pub(crate) struct UncertainRecovery<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) scope: &'a ConnectionJournal,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) image_authority: &'a ImageAuthority,
    pub(crate) pass: usize,
}

pub(crate) async fn recover_uncertain_reservations(
    recovery: &UncertainRecovery<'_>,
) -> Result<(), HostError> {
    let mut initializers = Vec::new();
    for reservation in recovery.scope.physical_runner_reservations().await? {
        if reservation.reservation.state != ReservationState::Uncertain
            || reservation.intent_state != Some(IntentState::Uncertain)
            || reservation.jit_state != PhysicalRunnerJitState::Absent
        {
            continue;
        }
        if recovery
            .scope
            .storage_initializer_abort_candidate(reservation.reservation_id)
            .await?
            .is_some()
        {
            initializers.push(reservation.reservation_id);
            continue;
        }
        recover_pre_jit(recovery, &reservation).await;
    }
    recover_initializers(recovery, &initializers).await;
    Ok(())
}

async fn recover_initializers(recovery: &UncertainRecovery<'_>, initializers: &[i64]) {
    recover_in_rotation(
        initializers,
        recovery.pass,
        INITIALIZER_RECOVERY_BUDGET,
        |id, remaining| async move {
            if let Err(error) = recover_absent(recovery.docker, recovery.scope, id, remaining).await
            {
                report(
                    recovery.scope,
                    id,
                    "an interrupted storage initializer",
                    error,
                );
            }
        },
    )
    .await;
}

async fn recover_in_rotation<Recover, Pending>(
    initializers: &[i64],
    pass: usize,
    budget: Duration,
    mut recover: Recover,
) where
    Recover: FnMut(i64, Duration) -> Pending,
    Pending: Future<Output = ()>,
{
    let deadline = Instant::now() + budget;
    let first = pass % initializers.len().max(1);
    for &id in initializers
        .iter()
        .cycle()
        .skip(first)
        .take(initializers.len())
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        recover(id, remaining).await;
    }
}

async fn recover_pre_jit(
    recovery: &UncertainRecovery<'_>,
    reservation: &PhysicalRunnerReservation,
) {
    if reservation.ownership.assignment().is_none() {
        return;
    }
    if reservation.acquired_request.is_some()
        && let Err(error) = Box::pin(recover_pre_jit_reservation(
            recovery.docker,
            recovery.journal,
            recovery.scope,
            recovery.engine,
            recovery.images,
            reservation.reservation_id,
            recovery.image_authority,
        ))
        .await
    {
        report(
            recovery.scope,
            reservation.reservation_id,
            "an uncertain pre-JIT reservation",
            error,
        );
    }
    if let Err(error) = Box::pin(retire_unbound_pair(
        recovery.docker,
        recovery.scope,
        recovery.engine,
        reservation.reservation_id,
    ))
    .await
    {
        report(
            recovery.scope,
            reservation.reservation_id,
            "an unbound pair",
            error,
        );
    }
}

fn report(scope: &ConnectionJournal, reservation_id: i64, work: &str, error: HostError) {
    eprintln!(
        "The controller keeps {work} charged for recovery. connection={} reservation={reservation_id} reason={error}",
        scope.connection_id().as_str()
    );
}

#[cfg(test)]
#[path = "uncertain_recovery/tests.rs"]
mod tests;
