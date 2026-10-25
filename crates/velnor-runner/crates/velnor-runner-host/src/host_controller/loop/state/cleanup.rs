use crate::error::HostError;

use super::PreparedConnection;
use crate::launch::scoped_runtime::{PendingScopedExecution, ScopedChannelOutcome};

pub(super) async fn collect_proven_pending_cleanup(
    connection: &mut PreparedConnection,
) -> Result<(), HostError> {
    collect_ready_cleanup(connection).await?;
    collect_bootstrap_cleanup(connection).await?;
    collect_authorization_cleanup(connection).await?;
    collect_acquired_cleanup(connection).await
}

async fn collect_ready_cleanup(connection: &mut PreparedConnection) -> Result<(), HostError> {
    let ready = connection
        .ready_intents
        .iter()
        .map(|(request_id, intent_id)| (*request_id, *intent_id))
        .collect::<Vec<_>>();
    for (request_id, intent_id) in ready {
        if connection.scope.intent(intent_id).await?.cleanup_proven
            && connection
                .runner_ready
                .take_ready_after_cleanup(request_id)
                .is_some()
        {
            connection.ready_intents.remove(&request_id);
        }
    }
    Ok(())
}

async fn collect_bootstrap_cleanup(connection: &mut PreparedConnection) -> Result<(), HostError> {
    let pending = connection
        .pending_bootstrap
        .iter()
        .map(|(request_id, values)| {
            (
                *request_id,
                values
                    .iter()
                    .map(ScopedChannelOutcome::intent_id)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    for (request_id, intents) in pending {
        let mut keep = Vec::with_capacity(intents.len());
        for intent_id in intents {
            keep.push(match intent_id {
                Some(intent_id) => !connection.scope.intent(intent_id).await?.cleanup_proven,
                None => true,
            });
        }
        retain_values(&mut connection.pending_bootstrap, request_id, keep);
    }
    let intents = connection
        .unkeyed_bootstrap
        .iter()
        .map(ScopedChannelOutcome::intent_id)
        .collect::<Vec<_>>();
    let mut keep = Vec::with_capacity(intents.len());
    for intent_id in intents {
        keep.push(match intent_id {
            Some(intent_id) => !connection.scope.intent(intent_id).await?.cleanup_proven,
            None => true,
        });
    }
    connection.unkeyed_bootstrap = std::mem::take(&mut connection.unkeyed_bootstrap)
        .into_iter()
        .zip(keep)
        .filter_map(|(value, keep)| keep.then_some(value))
        .collect();
    Ok(())
}

async fn collect_authorization_cleanup(
    connection: &mut PreparedConnection,
) -> Result<(), HostError> {
    let pending = connection
        .pending_authorization
        .iter()
        .map(|(request_id, values)| {
            (
                *request_id,
                values
                    .iter()
                    .map(PendingScopedExecution::intent_id)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    for (request_id, intents) in pending {
        let mut keep = Vec::with_capacity(intents.len());
        for intent_id in intents {
            keep.push(!connection.scope.intent(intent_id).await?.cleanup_proven);
        }
        retain_values(&mut connection.pending_authorization, request_id, keep);
    }
    let intents = connection
        .unkeyed_authorization
        .iter()
        .map(PendingScopedExecution::intent_id)
        .collect::<Vec<_>>();
    let mut keep = Vec::with_capacity(intents.len());
    for intent_id in intents {
        keep.push(!connection.scope.intent(intent_id).await?.cleanup_proven);
    }
    connection.unkeyed_authorization = std::mem::take(&mut connection.unkeyed_authorization)
        .into_iter()
        .zip(keep)
        .filter_map(|(value, keep)| keep.then_some(value))
        .collect();
    Ok(())
}

async fn collect_acquired_cleanup(connection: &mut PreparedConnection) -> Result<(), HostError> {
    let pending = connection
        .pending_acquired_recovery
        .iter()
        .map(|(request_id, value)| (*request_id, value.intent_id))
        .collect::<Vec<_>>();
    for (request_id, intent_id) in pending {
        if connection.scope.intent(intent_id).await?.cleanup_proven {
            connection.pending_acquired_recovery.remove(&request_id);
        }
    }
    let intents = connection
        .unkeyed_acquired_recovery
        .iter()
        .map(|value| value.intent_id)
        .collect::<Vec<_>>();
    let mut keep = Vec::with_capacity(intents.len());
    for intent_id in intents {
        keep.push(!connection.scope.intent(intent_id).await?.cleanup_proven);
    }
    connection.unkeyed_acquired_recovery =
        std::mem::take(&mut connection.unkeyed_acquired_recovery)
            .into_iter()
            .zip(keep)
            .filter_map(|(value, keep)| keep.then_some(value))
            .collect();
    Ok(())
}

fn retain_values<T>(
    values: &mut std::collections::BTreeMap<i64, Vec<T>>,
    request_id: i64,
    keep: Vec<bool>,
) {
    if let Some(entries) = values.get_mut(&request_id) {
        *entries = std::mem::take(entries)
            .into_iter()
            .zip(keep)
            .filter_map(|(entry, keep)| keep.then_some(entry))
            .collect();
        if entries.is_empty() {
            values.remove(&request_id);
        }
    }
}
