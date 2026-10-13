//! Read-only, deadline-bounded proof that a Linux daemon has drained.

use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::time::{Instant as TokioInstant, sleep, timeout_at};
use velnor_runner_host::worker::{
    OwnedDockerResource, ProtectedStateDirectory, list_owned_docker_resources_until,
};
use velnor_runner_journal::journal::Journal;

use super::control::{
    ControlOpenError, DrainOutcome, DrainRequestOutcome, DrainUnknown, blocking_runtime,
};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Persist the drain fence against the same retained state directory used by the daemon lock.
///
/// This opens only an existing journal and pins both its parent and database identity. It
/// never creates a database, bootstraps a schema, or follows a replacement state path.
///
/// # Errors
///
/// Returns [`ControlOpenError`] if the pinned journal or a safe blocking runtime is unavailable
/// before the mutation is dispatched.
pub fn request_drain_protected_blocking(
    protected_state: &ProtectedStateDirectory,
    journal_path: &Path,
    deadline: Instant,
) -> Result<DrainRequestOutcome, ControlOpenError> {
    if Instant::now() >= deadline {
        return Ok(DrainRequestOutcome::DeadlineBeforeMutation);
    }
    let identity = protected_state
        .identity()
        .map_err(|_| ControlOpenError::JournalUnavailable)?;
    let runtime = blocking_runtime()?;
    let tokio_deadline = TokioInstant::from_std(deadline);

    runtime.block_on(async {
        let journal = match timeout_at(
            tokio_deadline,
            Journal::open_existing_protected_at(journal_path, identity.device(), identity.inode()),
        )
        .await
        {
            Ok(Ok(journal)) => journal,
            Ok(Err(_)) => return Err(ControlOpenError::JournalUnavailable),
            Err(_) => return Ok(DrainRequestOutcome::DeadlineBeforeMutation),
        };
        if Instant::now() >= deadline {
            return Ok(DrainRequestOutcome::DeadlineBeforeMutation);
        }

        // Once dispatched, a timeout or error is ambiguous: the transaction may commit.
        if !matches!(
            timeout_at(tokio_deadline, journal.request_drain()).await,
            Ok(Ok(()))
        ) {
            return Ok(DrainRequestOutcome::UnknownAfterMutation);
        }
        if Instant::now() >= deadline {
            return Ok(DrainRequestOutcome::UnknownAfterMutation);
        }
        match timeout_at(tokio_deadline, journal.draining()).await {
            Ok(Ok(true)) if Instant::now() < deadline => Ok(DrainRequestOutcome::Requested),
            _ => Ok(DrainRequestOutcome::UnknownAfterMutation),
        }
    })
}

/// Wait for the durable fence, journal cleanup, and complete Docker inventory to agree.
///
/// The absolute deadline is shared by all journal and Docker observations. `Drained` is
/// returned only after a pinned journal reports the drain fence and zero occupied/unresolved
/// rows, the host returns a complete empty owned-resource inventory, and a final pinned
/// journal snapshot still reports the fence with zero counts. Incomplete observations remain
/// unknown; persistent work remains a deadline result.
#[must_use]
pub fn wait_drained_protected_blocking(
    protected_state: &ProtectedStateDirectory,
    journal_path: &Path,
    docker_endpoint: &str,
    deadline: Instant,
) -> DrainOutcome {
    wait_drained_protected_blocking_with(
        protected_state,
        journal_path,
        docker_endpoint,
        deadline,
        |endpoint, cutoff| async move {
            match timeout_at(cutoff, list_owned_docker_resources_until(&endpoint, cutoff)).await {
                Ok(Ok(resources)) => Ok(resources),
                _ => Err(()),
            }
        },
    )
}

fn wait_drained_protected_blocking_with<F, Fut>(
    protected_state: &ProtectedStateDirectory,
    journal_path: &Path,
    docker_endpoint: &str,
    deadline: Instant,
    inventory: F,
) -> DrainOutcome
where
    F: FnMut(String, TokioInstant) -> Fut,
    Fut: Future<Output = Result<Vec<OwnedDockerResource>, ()>>,
{
    if Instant::now() >= deadline {
        return DrainOutcome::Deadline;
    }
    let Ok(identity) = protected_state.identity() else {
        return DrainOutcome::Unknown(DrainUnknown::StateDirectoryUnavailable);
    };
    let runtime = match blocking_runtime() {
        Ok(runtime) => runtime,
        Err(ControlOpenError::JournalUnavailable) => {
            return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable);
        }
        Err(ControlOpenError::RuntimeUnavailable) => {
            return DrainOutcome::Unknown(DrainUnknown::RuntimeUnavailable);
        }
    };
    runtime.block_on(async {
        let tokio_deadline = TokioInstant::from_std(deadline);
        let journal = match timeout_at(
            tokio_deadline,
            Journal::open_readonly_protected_at(journal_path, identity.device(), identity.inode()),
        )
        .await
        {
            Ok(Ok(journal)) => journal,
            Ok(Err(_)) => return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable),
            Err(_) => return DrainOutcome::Deadline,
        };
        observe_until_deadline(&journal, docker_endpoint, deadline, inventory).await
    })
}

async fn observe_until_deadline<F, Fut>(
    journal: &Journal,
    docker_endpoint: &str,
    deadline: Instant,
    mut inventory: F,
) -> DrainOutcome
where
    F: FnMut(String, TokioInstant) -> Fut,
    Fut: Future<Output = Result<Vec<OwnedDockerResource>, ()>>,
{
    let cutoff = TokioInstant::from_std(deadline);
    loop {
        if Instant::now() >= deadline {
            return DrainOutcome::Deadline;
        }
        let snapshot = match timeout_at(cutoff, journal.drain_snapshot()).await {
            Ok(Ok(snapshot)) => snapshot,
            Ok(Err(_)) => return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable),
            Err(_) => return DrainOutcome::Deadline,
        };
        if !snapshot.draining {
            return DrainOutcome::Unknown(DrainUnknown::AdmissionNotFenced);
        }
        if snapshot.occupied_launches == 0 && snapshot.unresolved_intents == 0 {
            let resources =
                match timeout_at(cutoff, inventory(docker_endpoint.to_owned(), cutoff)).await {
                    Ok(Ok(resources)) => resources,
                    Ok(Err(())) => {
                        return DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable);
                    }
                    Err(_) => return DrainOutcome::Deadline,
                };
            if Instant::now() >= deadline {
                return DrainOutcome::Deadline;
            }
            if resources.is_empty() {
                let final_snapshot = match timeout_at(cutoff, journal.drain_snapshot()).await {
                    Ok(Ok(snapshot)) => snapshot,
                    Ok(Err(_)) => {
                        return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable);
                    }
                    Err(_) => return DrainOutcome::Deadline,
                };
                if !final_snapshot.draining {
                    return DrainOutcome::Unknown(DrainUnknown::AdmissionNotFenced);
                }
                if final_snapshot.occupied_launches == 0
                    && final_snapshot.unresolved_intents == 0
                    && Instant::now() < deadline
                {
                    return DrainOutcome::Drained;
                }
            }
        }
        if timeout_at(cutoff, sleep(POLL_INTERVAL)).await.is_err() {
            return DrainOutcome::Deadline;
        }
    }
}

#[cfg(test)]
mod tests;
