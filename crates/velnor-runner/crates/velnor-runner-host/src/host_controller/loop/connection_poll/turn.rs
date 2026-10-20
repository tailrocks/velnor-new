//! Poll one connection.
//! Remove its session when its work is complete.

use crate::{
    config::RepositoryConnection,
    error::HostError,
    journal::HeaderAllocation,
    launch::{SessionDeleteOutcome, delete_removed_connection_session},
};

use super::super::{
    polling::{
        ControllerPollOutcome, PositivePollContext, poll_positive_connection,
        poll_zero_header_connection,
    },
    state::{AuthorizationContext, PreparedConnection},
};
use super::{
    ConnectionPollContext,
    fairness::{
        BindingDisposition, DeleteReadiness, ExecutionDisposition, FairnessTurn, HeaderPollPlan,
    },
};

pub(super) struct PollTurnResult {
    pub(super) remove_connection: bool,
    pub(super) positive_poll_settled: bool,
}

struct PreparedPoll {
    allocation: HeaderAllocation,
    is_removed: bool,
    drained_before_poll: bool,
}

pub(super) async fn poll_connection(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
    turn: &mut FairnessTurn,
) -> Result<Option<PollTurnResult>, HostError> {
    prepared.collect_authorization().await?;
    let Some(poll) = prepare_poll(prepared, context, pending_retirements, turn).await? else {
        return Ok(None);
    };
    let Some(outcome) = execute_poll(prepared, context, pending_retirements, turn, &poll).await?
    else {
        return Ok(Some(PollTurnResult {
            remove_connection: false,
            positive_poll_settled: false,
        }));
    };
    if outcome.positive_poll_settled {
        turn.observe_positive_poll(true);
    }
    reconcile_poll(prepared, context, poll.is_removed, &outcome).await?;
    let remove_connection = delete_session_if_ready(prepared, turn, poll, &outcome).await?;
    if !remove_connection {
        start_ready_authorizations(prepared, context, outcome.started_request_ids)?;
    }
    Ok(Some(PollTurnResult {
        remove_connection,
        positive_poll_settled: outcome.positive_poll_settled,
    }))
}

async fn prepare_poll(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
    turn: &FairnessTurn,
) -> Result<Option<PreparedPoll>, HostError> {
    let is_removed = context
        .config
        .connection(&prepared.binding.connection_id)
        .is_none();
    if is_removed
        && !pending_retirements
            .iter()
            .any(|binding| prepared.is_for(binding))
    {
        return Ok(None);
    }
    if prepared
        .guest_image
        .runner_bind(&prepared.binding, context.worker_images)
        .is_err()
    {
        eprintln!(
            "host controller guest image metadata held connection={}",
            prepared.binding.connection_id
        );
        prepared.credentials.take();
        return Ok(None);
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
            return Ok(None);
        }
    };
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
                return Ok(None);
            }
        }
    } else {
        false
    };
    Ok(Some(PreparedPoll {
        allocation,
        is_removed,
        drained_before_poll,
    }))
}

async fn execute_poll(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
    turn: &FairnessTurn,
    poll: &PreparedPoll,
) -> Result<Option<ControllerPollOutcome>, HostError> {
    let poll_context = positive_poll_context(context);
    let plan = turn.poll_plan(poll.is_removed, poll.allocation.target(), || {
        prepared.cursor_for_poll(std::time::Instant::now())
    });
    let outcome = match plan {
        HeaderPollPlan::Positive => Box::pin(poll_positive_connection(
            prepared,
            &poll.allocation,
            &poll_context,
        ))
        .await
        .map_err(|_| HostError::Journal),
        HeaderPollPlan::Zero { cursor } => {
            let retired_binding = pending_retirements
                .iter()
                .find(|binding| prepared.is_for(binding));
            poll_zero_header_connection(
                prepared,
                &poll.allocation,
                &poll_context,
                cursor,
                turn.retiring() || poll.is_removed,
                retired_binding,
            )
            .await
            .map(|outcome| {
                prepared.record_cursor_disposition(
                    outcome.message_id,
                    outcome.cursor_disposition,
                    std::time::Instant::now(),
                );
                ControllerPollOutcome::from_scoped(outcome, cursor)
            })
            .map_err(|_| HostError::Journal)
        }
    };
    match outcome {
        Ok(outcome) => Ok(Some(outcome)),
        Err(error) => {
            eprintln!(
                "host controller poll held connection={} reason={error}",
                prepared.binding.connection_id
            );
            prepared.credentials.take();
            Ok(None)
        }
    }
}

async fn reconcile_poll(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    is_removed: bool,
    outcome: &ControllerPollOutcome,
) -> Result<(), HostError> {
    prepared.record_assigned_total(outcome.assigned_total);
    if !is_removed {
        Box::pin(super::super::polling::reconcile_assigned_population(
            prepared,
            &positive_poll_context(context),
        ))
        .await?;
    }
    if outcome.wake_cleanup
        && let Some(completion) = prepared.completion.as_ref()
    {
        completion.notify();
    }
    Ok(())
}

async fn delete_session_if_ready(
    prepared: &mut PreparedConnection,
    turn: &FairnessTurn,
    poll: PreparedPoll,
    outcome: &ControllerPollOutcome,
) -> Result<bool, HostError> {
    let readiness = DeleteReadiness {
        binding: if poll.is_removed {
            BindingDisposition::Removed
        } else {
            BindingDisposition::Active
        },
        poll_kind: outcome.poll_kind,
        drained: poll.drained_before_poll,
        candidate: outcome.session_delete_candidate,
        execution: if prepared.has_live_execution_state() {
            ExecutionDisposition::Live
        } else {
            ExecutionDisposition::Idle
        },
    };
    if !turn.should_delete(&readiness) {
        return Ok(false);
    }
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let deleted = match delete_removed_connection_session(
        &mut credentials.link,
        &credentials.session,
        &prepared.scope,
        prepared.session_intent,
        &poll.allocation,
    )
    .await
    {
        SessionDeleteOutcome::NotReady => return Ok(false),
        SessionDeleteOutcome::Deleted => true,
        SessionDeleteOutcome::Held => {
            // DELETE may have taken effect without a returned 204 response.
            // Keep its capacity reservation and stop use of this credential.
            false
        }
    };
    prepared.credentials.take();
    shutdown_completion(prepared).await;
    Ok(deleted)
}

async fn shutdown_completion(prepared: &mut PreparedConnection) {
    if let Some(completion) = prepared.completion.take()
        && let Err(error) = completion.shutdown().await
    {
        eprintln!(
            "host controller completion shutdown held connection={} reason={error}",
            prepared.binding.connection_id
        );
    }
}

fn start_ready_authorizations(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    started_request_ids: Vec<i64>,
) -> Result<(), HostError> {
    let started_requests = prepared.runner_ready.record_started(started_request_ids);
    for request_id in started_requests {
        let Some(runner) = prepared.runner_ready.take_started(request_id) else {
            continue;
        };
        prepared.ready_intents.remove(&request_id);
        prepared.start_authorization(
            request_id,
            runner,
            AuthorizationContext {
                config: context.config,
                journal: context.journal,
                docker: context.docker,
                selected_engine: context.selected_engine,
                worker_images: context.worker_images,
            },
        )?;
    }
    Ok(())
}

fn positive_poll_context<'a>(context: &ConnectionPollContext<'a>) -> PositivePollContext<'a> {
    PositivePollContext {
        config: context.config,
        journal: context.journal,
        docker: context.docker,
        selected_engine: context.selected_engine,
        worker_images: context.worker_images,
        state: context.state,
        release_read_secret: context.release_read_secret,
        admission_ready: context.admission_ready,
    }
}
