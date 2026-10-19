use crate::{
    error::HostError,
    launch::scoped_runtime::{PendingScopedExecution, RunnerReadyBootstrap, ScopedChannelOutcome},
};

use super::super::{PendingAcquiredRecovery, state::PreparedConnection};

pub(super) fn retain_ready_or_pending(
    prepared: &mut PreparedConnection,
    runner: RunnerReadyBootstrap,
) {
    let request_id = runner.request_id();
    match prepared.retain_runner_ready(runner) {
        Ok(_) => clear_acquired_recovery(prepared, request_id),
        Err(runner) => retain_authorization_pending(
            prepared,
            request_id,
            PendingScopedExecution::Runner(runner, None, HostError::Ownership),
        ),
    }
}

pub(super) fn retain_authorization_pending(
    prepared: &mut PreparedConnection,
    request_id: Option<i64>,
    pending: PendingScopedExecution,
) {
    if let Some(request_id) = request_id {
        prepared.retain_pending(request_id, pending);
        clear_acquired_recovery(prepared, Some(request_id));
    } else {
        prepared.unkeyed_authorization.push(pending);
        prepared.unkeyed_acquired_recovery.pop();
    }
}

pub(super) fn retain_bootstrap_pending(
    prepared: &mut PreparedConnection,
    request_id: Option<i64>,
    pending: ScopedChannelOutcome,
) {
    if let Some(request_id) = request_id {
        prepared
            .pending_bootstrap
            .entry(request_id)
            .or_default()
            .push(pending);
        clear_acquired_recovery(prepared, Some(request_id));
    } else {
        prepared.unkeyed_bootstrap.push(pending);
        prepared.unkeyed_acquired_recovery.pop();
    }
}

pub(super) fn remember_acquired_recovery(
    prepared: &mut PreparedConnection,
    request_id: Option<i64>,
    acquired: &crate::launch::steps::AcquiredProvisioning,
) {
    let recovery = PendingAcquiredRecovery {
        reservation_id: acquired.reservation_id(),
        intent_id: acquired.intent_id(),
        failure: HostError::Journal,
    };
    if let Some(request_id) = request_id {
        prepared
            .pending_acquired_recovery
            .entry(request_id)
            .or_insert(recovery);
    } else {
        prepared.unkeyed_acquired_recovery.push(recovery);
    }
}

pub(super) fn report_acquired_recovery(
    prepared: &mut PreparedConnection,
    request_id: Option<i64>,
    error: HostError,
) {
    let recovery = request_id
        .and_then(|request_id| prepared.pending_acquired_recovery.get_mut(&request_id))
        .or_else(|| prepared.unkeyed_acquired_recovery.last_mut());
    if let Some(recovery) = recovery {
        recovery.failure = error;
        eprintln!(
            "Acquired work remains held for recovery. reservation={} intent={} reason={}",
            recovery.reservation_id, recovery.intent_id, recovery.failure
        );
    }
}

pub(super) fn clear_acquired_recovery(prepared: &mut PreparedConnection, request_id: Option<i64>) {
    if let Some(request_id) = request_id {
        prepared.pending_acquired_recovery.remove(&request_id);
    } else {
        prepared.unkeyed_acquired_recovery.pop();
    }
}
