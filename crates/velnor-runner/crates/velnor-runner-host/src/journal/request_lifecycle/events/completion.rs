//! Record completed request events and their cleanup records.

use super::{
    AssignmentIdentity, CompletedRecord, ConnectionJournal, HostError, bind_completed_pair,
    bound_set_id, insert_completion_cleanup, matching_runner_intent, request_event_state,
    require_one_change, source_receipt_matches, validate_job_id_history, validate_scope_identity,
};

pub(super) async fn record_completed(
    journal: &ConnectionJournal,
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
    record: &CompletedRecord<'_>,
) -> Result<Option<i64>, HostError> {
    validate_scope_identity(journal, connection, identity).await?;
    let set_id = bound_set_id(connection, &record.connection_id).await?;
    source_receipt_matches(
        journal,
        connection,
        record.connection_id.as_str(),
        set_id,
        record.request_id,
        record.source,
    )
    .await?;
    let Some(state) =
        request_event_state(connection, &record.connection_id, set_id, record.request_id).await?
    else {
        return Err(HostError::Journal);
    };
    validate_job_id_history(&state, record.job_id)?;
    if let Some(existing) = state.completed_digest.as_deref() {
        return if existing == record.event_digest {
            Ok(state.intent_id)
        } else {
            Err(HostError::Identity)
        };
    }
    validate_completion_state(&state.acquire_state, record.runner.is_some())?;
    let pair = completed_pair(connection, record, set_id).await?;
    if let Some((intent_id, runner_id, runner_name)) = pair {
        bind_completed_pair(
            connection,
            &record.connection_id,
            set_id,
            record.request_id,
            intent_id,
            runner_id,
            runner_name,
        )
        .await?;
    }
    record_completed_authorization(connection, record, set_id, pair).await?;
    if let Some((intent_id, runner_id, runner_name)) = pair {
        record_completed_jit_and_cleanup(
            connection,
            record,
            set_id,
            intent_id,
            runner_id,
            runner_name,
        )
        .await?;
    }
    Ok(pair.map(|(intent_id, _, _)| intent_id))
}

fn validate_completion_state(state: &str, has_runner: bool) -> Result<(), HostError> {
    if has_runner && state != "acquired" {
        return Err(HostError::Journal);
    }
    if !has_runner
        && !matches!(
            state,
            "authorized" | "requested" | "acquired" | "already_acquired" | "uncertain"
        )
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn completed_pair<'a>(
    connection: &turso::Connection,
    record: &'a CompletedRecord<'a>,
    set_id: i64,
) -> Result<Option<(i64, i64, &'a str)>, HostError> {
    let Some(runner) = record.runner else {
        return Ok(None);
    };
    if runner.runner_scale_set_id != set_id {
        return Err(HostError::Identity);
    }
    let intent_id = matching_runner_intent(
        connection,
        &record.connection_id,
        set_id,
        runner.id,
        &runner.name,
        record.request_id,
        false,
    )
    .await?;
    Ok(Some((intent_id, runner.id, runner.name.as_str())))
}

async fn record_completed_authorization(
    connection: &turso::Connection,
    record: &CompletedRecord<'_>,
    set_id: i64,
    pair: Option<(i64, i64, &str)>,
) -> Result<(), HostError> {
    let (runner_id, runner_name, intent_id) = pair
        .map_or((None, None, None), |(intent, id, name)| {
            (Some(id), Some(name), Some(intent))
        });
    let changed = connection.execute(
        "UPDATE connection_request_authorizations SET completed_message_id = ?1, completed_event_digest = ?2, completed_job_id = ?3, completed_result = ?4, completed_runner_id = ?5, completed_runner_name = ?6, completed_intent_id = ?7 WHERE connection_id = ?8 AND scale_set_id = ?9 AND request_id = ?10 AND completed_event_digest IS NULL AND (started_event_digest IS NULL OR ?5 IS NULL OR (started_runner_id = ?5 AND started_runner_name = ?6))",
        (record.message_id, record.event_digest.as_str(), record.job_id, record.result, runner_id, runner_name, intent_id, record.connection_id.as_str(), set_id, record.request_id),
    ).await.map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn record_completed_jit_and_cleanup(
    connection: &turso::Connection,
    record: &CompletedRecord<'_>,
    set_id: i64,
    intent_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<(), HostError> {
    let changed = connection.execute(
        "UPDATE connection_runner_jits SET completed_request_id = ?1, state = 'completed' WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND runner_id = ?5 AND runner_name = ?6 AND state IN ('bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed') AND (completed_request_id IS NULL OR completed_request_id = ?1) AND (started_request_id IS NULL OR started_request_id = ?1)",
        (record.request_id, intent_id, record.connection_id.as_str(), set_id, runner_id, runner_name),
    ).await.map_err(|_| HostError::Journal)?;
    require_one_change(changed)?;
    insert_completion_cleanup(
        connection,
        &record.connection_id,
        set_id,
        record.request_id,
        intent_id,
        runner_id,
        runner_name,
    )
    .await
}
