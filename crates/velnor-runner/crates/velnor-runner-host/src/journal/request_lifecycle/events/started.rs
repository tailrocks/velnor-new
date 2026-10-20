//! Record exact Started events without permitting execution after a retirement claim.

use super::storage::RequestEventState;
use super::{
    AssignmentIdentity, ConnectionJournal, HostError, RecordedEventDisposition, StartedRecord,
    bound_set_id, exact_started_intent, matching_runner_intent, record_started_authorization,
    record_started_intent, record_started_jit, request_completed_runnerless, request_event_state,
    source_receipt_matches, validate_job_id_history, validate_scope_identity,
};
use crate::journal::connection::reservation::post_jit_retirement::{
    EventRetirementDisposition, disposition_for_event_in_transaction,
};

struct StartedContext {
    set_id: i64,
    state: RequestEventState,
    retirement: Option<EventRetirementDisposition>,
}

pub(super) async fn record_started(
    journal: &ConnectionJournal,
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
    record: &StartedRecord<'_>,
) -> Result<RecordedEventDisposition, HostError> {
    let context = prepare_started(journal, connection, identity, record).await?;
    let existing = context.state.started_digest.clone();
    if let Some(existing) = existing.as_deref() {
        return replay_started(connection, record, context, existing).await;
    }
    if context.retirement.is_some() {
        return record_historical_started(connection, record, context).await;
    }
    record_active_started(connection, record, context).await
}

async fn prepare_started(
    journal: &ConnectionJournal,
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
    record: &StartedRecord<'_>,
) -> Result<StartedContext, HostError> {
    validate_scope_identity(journal, connection, identity).await?;
    let set_id = bound_set_id(connection, &record.connection_id).await?;
    if set_id != record.runner.runner_scale_set_id {
        return Err(HostError::Identity);
    }
    source_receipt_matches(
        journal,
        connection,
        record.connection_id.as_str(),
        set_id,
        record.request_id,
        record.source,
    )
    .await?;
    let state = request_event_state(connection, &record.connection_id, set_id, record.request_id)
        .await?
        .ok_or(HostError::Journal)?;
    validate_job_id_history(&state, record.job_id)?;
    let retirement = disposition_for_event_in_transaction(
        connection,
        journal,
        set_id,
        record.request_id,
        record.runner.id,
        record.runner.name.as_str(),
    )
    .await?;
    Ok(StartedContext {
        set_id,
        state,
        retirement,
    })
}

async fn replay_started(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    context: StartedContext,
    existing: &str,
) -> Result<RecordedEventDisposition, HostError> {
    if existing != record.event_digest {
        return Err(HostError::Identity);
    }
    if let Some(retirement) = context.retirement {
        return Ok(RecordedEventDisposition::Historical {
            intent_id: super::historical_intent_id(retirement),
        });
    }
    exact_started_intent(
        connection,
        &record.connection_id,
        context.set_id,
        record.request_id,
        record.runner,
    )
    .await
    .map(|intent_id| RecordedEventDisposition::Active {
        intent_id: Some(intent_id),
    })
}

async fn record_historical_started(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    context: StartedContext,
) -> Result<RecordedEventDisposition, HostError> {
    let StartedContext {
        set_id,
        state,
        retirement,
    } = context;
    let Some(retirement) = retirement else {
        return Err(HostError::Journal);
    };
    validate_started_state(connection, record, &state, set_id).await?;
    let intent_id = super::historical_intent_id(retirement);
    record_started_authorization(connection, record, set_id, intent_id).await?;
    Ok(RecordedEventDisposition::Historical { intent_id })
}

async fn record_active_started(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    context: StartedContext,
) -> Result<RecordedEventDisposition, HostError> {
    validate_started_state(connection, record, &context.state, context.set_id).await?;
    let intent_id = matching_runner_intent(
        connection,
        &record.connection_id,
        context.set_id,
        record.runner.id,
        record.runner.name.as_str(),
        record.request_id,
        true,
    )
    .await?;
    record_started_authorization(connection, record, context.set_id, intent_id).await?;
    record_started_jit(connection, record, context.set_id, intent_id).await?;
    record_started_intent(connection, record, context.set_id, intent_id).await?;
    Ok(RecordedEventDisposition::Active {
        intent_id: Some(intent_id),
    })
}

async fn validate_started_state(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    state: &RequestEventState,
    set_id: i64,
) -> Result<(), HostError> {
    if state.acquire_state != "acquired"
        || request_completed_runnerless(
            connection,
            &record.connection_id,
            set_id,
            record.request_id,
        )
        .await?
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
