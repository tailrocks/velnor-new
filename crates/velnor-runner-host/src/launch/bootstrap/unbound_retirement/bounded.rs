// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use tokio::time::{Instant, timeout_at};

use crate::{error::HostError, journal::ConnectionJournal, launch::SelectedEngineContext};

use super::{abandon_and_retire_unbound_pair, retire_unbound_pair};

pub(crate) async fn abandon_and_retire_unbound_pair_before(
    deadline: Instant,
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<bool, HostError> {
    timeout_at(
        deadline,
        abandon_and_retire_unbound_pair(docker, scope, engine, reservation_id),
    )
    .await
    .map_err(|_| HostError::Docker)?
}

pub(crate) async fn retire_uncertain_unbound_pairs(
    deadline: Instant,
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    for reservation_id in scope.uncertain_unbound_pairs().await? {
        if !retire_unbound_pair_before(deadline, docker, scope, engine, reservation_id).await {
            break;
        }
    }
    Ok(())
}

pub(crate) async fn retire_unbound_pair_before(
    deadline: Instant,
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> bool {
    if Instant::now() >= deadline {
        return false;
    }
    let retired = timeout_at(
        deadline,
        Box::pin(retire_unbound_pair(docker, scope, engine, reservation_id)),
    )
    .await;
    match retired {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => {
            eprintln!(
                "The controller keeps an unbound pair charged for retirement. connection={} reservation={reservation_id} reason={error}",
                scope.connection_id().as_str()
            );
            true
        }
        Err(_) => {
            eprintln!(
                "The controller defers unbound pair retirement to the next turn. connection={} reservation={reservation_id}",
                scope.connection_id().as_str()
            );
            false
        }
    }
}
