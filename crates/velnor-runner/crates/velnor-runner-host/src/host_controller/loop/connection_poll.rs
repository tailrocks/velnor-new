//! Poll active Scale Set sessions and reconcile their event handoffs.

use std::path::Path;

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::Journal,
    launch::{
        ReleaseReadSecret, SelectedEngineContext, SessionDeleteOutcome, VerifiedWorkerImages,
        delete_removed_connection_session,
    },
};

use super::{
    polling::{
        ControllerPollOutcome, PositivePollContext, poll_positive_connection,
        poll_zero_header_connection,
    },
    state::PreparedConnection,
};

#[path = "connection_poll/fairness.rs"]
pub(super) mod fairness;

use fairness::{FairnessTurn, HeaderPollPlan};

pub(super) async fn poll_active_connections(
    config: &HostConfig,
    pending_retirements: &[RepositoryConnection],
    connections: &mut Vec<PreparedConnection>,
    journal: &Journal,
    docker: &Docker,
    selected_engine: &SelectedEngineContext,
    worker_images: &VerifiedWorkerImages,
    state: &Path,
    release_read_secret: &ReleaseReadSecret,
    admission_ready: bool,
    fairness_enabled: bool,
    rotate: bool,
) -> Result<ConnectionPollOutcome, HostError> {
    let mut result = ConnectionPollOutcome::default();
    let mut turn = FairnessTurn::new(fairness_enabled, rotate);
    let mut index = 0;
    while index < connections.len() {
        let prepared = &mut connections[index];
        prepared.collect_authorization().await?;
        let is_removed = config.connection(&prepared.binding.connection_id).is_none();
        if is_removed
            && !pending_retirements
                .iter()
                .any(|binding| prepared.is_for(binding))
        {
            index += 1;
            continue;
        }
        if prepared
            .guest_image
            .runner_bind(&prepared.binding, worker_images)
            .is_err()
        {
            eprintln!(
                "host controller guest image metadata held connection={}",
                prepared.binding.connection_id
            );
            prepared.credentials.take();
            index += 1;
            continue;
        }
        let allocation = match prepared
            .scope
            .session_header_allocation(prepared.session_intent)
            .await
        {
            Ok(allocation) => allocation,
            Err(error) => {
                eprintln!(
                    "host controller polling held connection={} reason={error}",
                    prepared.binding.connection_id
                );
                prepared.credentials.take();
                index += 1;
                continue;
            }
        };
        // A removed binding must pass the read-only drain check before its
        // final zero poll. The delete permission operation does this check again in the same transaction.
        let drained_before_poll = if is_removed || turn.retiring() {
            match prepared
                .scope
                .session_work_drained(prepared.session_intent)
                .await
            {
                Ok(drained) => drained,
                Err(error) => {
                    eprintln!(
                        "host controller drain check held connection={} reason={error}",
                        prepared.binding.connection_id
                    );
                    prepared.credentials.take();
                    index += 1;
                    continue;
                }
            }
        } else {
            false
        };
        let poll_plan = turn.poll_plan(is_removed, allocation.target(), || {
            prepared.cursor_for_poll(std::time::Instant::now())
        });
        let outcome = if poll_plan == HeaderPollPlan::Positive {
            let poll_context = PositivePollContext {
                config,
                journal,
                docker,
                selected_engine,
                worker_images,
                state,
                release_read_secret,
                admission_ready,
            };
            Box::pin(poll_positive_connection(
                prepared,
                &allocation,
                &poll_context,
            ))
            .await
            .map_err(|_| HostError::Journal)
        } else {
            let HeaderPollPlan::Zero { cursor } = poll_plan else {
                return Err(HostError::Journal);
            };
            let poll_context = PositivePollContext {
                config,
                journal,
                docker,
                selected_engine,
                worker_images,
                state,
                release_read_secret,
                admission_ready,
            };
            let retired_binding = pending_retirements
                .iter()
                .find(|binding| prepared.is_for(binding));
            let polled = poll_zero_header_connection(
                prepared,
                &allocation,
                &poll_context,
                cursor,
                turn.retiring() || is_removed,
                retired_binding,
            )
            .await;
            polled
                .map(|outcome| {
                    prepared.record_cursor_disposition(
                        outcome.message_id,
                        outcome.cursor_disposition,
                        std::time::Instant::now(),
                    );
                    ControllerPollOutcome::from_scoped(outcome, cursor)
                })
                .map_err(|_| HostError::Journal)
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!(
                    "host controller poll held connection={} reason={error}",
                    prepared.binding.connection_id
                );
                prepared.credentials.take();
                index += 1;
                continue;
            }
        };
        if outcome.positive_poll_settled {
            result.positive_poll_settled = true;
            turn.observe_positive_poll(true);
        }
        let started_request_ids = outcome.started_request_ids;
        prepared.record_assigned_total(outcome.assigned_total);
        let population_context = PositivePollContext {
            config,
            journal,
            docker,
            selected_engine,
            worker_images,
            state,
            release_read_secret,
            admission_ready,
        };
        if !is_removed {
            super::polling::reconcile_assigned_population(prepared, &population_context).await?;
        }
        if outcome.wake_cleanup
            && let Some(completion) = prepared.completion.as_ref()
        {
            completion.notify();
        }

        let should_delete = turn.should_delete(
            is_removed,
            outcome.poll_kind,
            drained_before_poll,
            outcome.session_delete_candidate,
            !prepared.has_live_execution_state(),
        );
        let mut remove_prepared = false;
        if should_delete {
            let Some(credentials) = prepared.credentials.as_mut() else {
                return Err(HostError::Keychain);
            };
            match delete_removed_connection_session(
                &mut credentials.link,
                &credentials.session,
                &prepared.scope,
                prepared.session_intent,
                &allocation,
            )
            .await
            {
                SessionDeleteOutcome::NotReady => {}
                SessionDeleteOutcome::Deleted => {
                    prepared.credentials.take();
                    if let Some(completion) = prepared.completion.take()
                        && let Err(error) = completion.shutdown().await
                    {
                        eprintln!(
                            "host controller completion shutdown held connection={} reason={error}",
                            prepared.binding.connection_id
                        );
                    }
                    remove_prepared = true;
                }
                SessionDeleteOutcome::Held => {
                    // DELETE may have taken effect without a returned 204
                    // response. Keep its capacity reservation. Stop all use
                    // of this session credential.
                    prepared.credentials.take();
                    if let Some(completion) = prepared.completion.take()
                        && let Err(error) = completion.shutdown().await
                    {
                        eprintln!(
                            "host controller completion shutdown held connection={} reason={error}",
                            prepared.binding.connection_id
                        );
                    }
                }
            }
        }
        if remove_prepared {
            connections.remove(index);
        } else {
            let prepared = &mut connections[index];
            let started_requests = prepared.runner_ready.record_started(started_request_ids);
            for request_id in started_requests {
                let Some(runner) = prepared.runner_ready.take_started(request_id) else {
                    continue;
                };
                prepared.ready_intents.remove(&request_id);
                prepared.start_authorization(
                    request_id,
                    runner,
                    config,
                    journal,
                    docker,
                    selected_engine,
                    worker_images,
                )?;
            }
            index += 1;
        }
    }
    result.rotation_complete = turn.rotation_complete(connections.is_empty());
    Ok(result)
}

#[derive(Default)]
pub(super) struct ConnectionPollOutcome {
    pub(super) positive_poll_settled: bool,
    pub(super) rotation_complete: bool,
}
