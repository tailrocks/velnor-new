//! Exact Started and Completed event correlation to charged physical runners.

use serde::Serialize;

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::journal::source_authorization::EventSource;
use velnor_runner_github::{RunnerReference, SourceMetadata};

use super::helpers::{bound_set_id, digest, request_id, validate_scope_identity};

const MAX_EVENT_TEXT: usize = 256;

#[derive(Serialize)]
struct RunnerEvent<'a> {
    request_id: i64,
    message_id: i64,
    runner_id: i64,
    runner_name: &'a str,
    scale_set_id: i64,
    job_id: Option<&'a str>,
    source: EventSource<'a>,
}

#[derive(Serialize)]
struct CompletionEvent<'a> {
    request_id: i64,
    message_id: i64,
    result: Option<&'a str>,
    runner: Option<RunnerEvent<'a>>,
    job_id: Option<&'a str>,
    source: EventSource<'a>,
}

struct StartedRecord<'a> {
    request_id: i64,
    message_id: i64,
    connection_id: String,
    source: EventSource<'a>,
    event_digest: String,
    runner: &'a RunnerReference,
    job_id: Option<&'a str>,
}

struct CompletedRecord<'a> {
    request_id: i64,
    message_id: i64,
    connection_id: String,
    source: EventSource<'a>,
    event_digest: String,
    result: Option<&'a str>,
    runner: Option<&'a RunnerReference>,
    job_id: Option<&'a str>,
}

impl ConnectionJournal {
    /// Correlate an actual `JobStarted` request with its exact JIT-created runner.
    pub(crate) async fn record_job_started(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        source: &SourceMetadata,
        runner: &RunnerReference,
        job_id: Option<&str>,
    ) -> Result<i64, HostError> {
        let request_id = request_id(identity, self)?;
        validate_event(message_id, runner, job_id)?;
        let source = validate_source(source)?;
        let record = StartedRecord {
            request_id,
            message_id,
            connection_id: self.connection_id().as_str().to_owned(),
            source,
            event_digest: digest(
                &serde_json::to_vec(&RunnerEvent {
                    request_id,
                    message_id,
                    runner_id: runner.id,
                    runner_name: &runner.name,
                    scale_set_id: runner.runner_scale_set_id,
                    job_id,
                    source,
                })
                .map_err(|_| HostError::Journal)?,
            ),
            runner,
            job_id,
        };
        in_transaction(self.journal_ref(), async |connection| {
            record_started(self, connection, identity, &record).await
        })
        .await
    }

    /// Persist a request completion; only an exact runner reference links a pair.
    pub(crate) async fn record_job_completed(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        source: &SourceMetadata,
        result: Option<&str>,
        runner: Option<&RunnerReference>,
        job_id: Option<&str>,
    ) -> Result<Option<i64>, HostError> {
        let request_id = request_id(identity, self)?;
        let source = validate_source(source)?;
        validate_optional_event_text(result)?;
        validate_optional_event_text(job_id)?;
        if message_id < 0 {
            return Err(HostError::Identity);
        }
        if let Some(runner) = runner {
            validate_runner(runner)?;
        }
        let event = CompletionEvent {
            request_id,
            message_id,
            result,
            runner: runner.map(|value| RunnerEvent {
                request_id,
                message_id,
                runner_id: value.id,
                runner_name: &value.name,
                scale_set_id: value.runner_scale_set_id,
                job_id,
                source,
            }),
            job_id,
            source,
        };
        let record = CompletedRecord {
            request_id,
            message_id,
            connection_id: self.connection_id().as_str().to_owned(),
            source,
            event_digest: digest(&serde_json::to_vec(&event).map_err(|_| HostError::Journal)?),
            result,
            runner,
            job_id,
        };
        in_transaction(self.journal_ref(), async |connection| {
            record_completed(self, connection, identity, &record).await
        })
        .await
    }
}

async fn record_started(
    journal: &ConnectionJournal,
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
    record: &StartedRecord<'_>,
) -> Result<i64, HostError> {
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
    let Some(state) =
        request_event_state(connection, &record.connection_id, set_id, record.request_id).await?
    else {
        return Err(HostError::Journal);
    };
    validate_job_id_history(&state, record.job_id)?;
    if let Some(existing) = state.started_digest.as_deref() {
        return if existing == record.event_digest {
            exact_started_intent(
                connection,
                &record.connection_id,
                set_id,
                record.request_id,
                record.runner,
            )
            .await
        } else {
            Err(HostError::Identity)
        };
    }
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
    let intent_id = matching_runner_intent(
        connection,
        &record.connection_id,
        set_id,
        record.runner.id,
        &record.runner.name,
        record.request_id,
        true,
    )
    .await?;
    record_started_authorization(connection, record, set_id, intent_id).await?;
    record_started_jit(connection, record, set_id, intent_id).await?;
    record_started_intent(connection, record, set_id, intent_id).await?;
    Ok(intent_id)
}

async fn source_receipt_matches(
    journal: &ConnectionJournal,
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
    source: EventSource<'_>,
) -> Result<(), HostError> {
    if crate::journal::source_authorization::event_source_matches(
        connection,
        connection_id,
        set_id,
        request_id,
        journal.binding_digest(),
        source,
    )
    .await?
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn record_started_authorization(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    set_id: i64,
    intent_id: i64,
) -> Result<(), HostError> {
    let changed = connection.execute(
        "UPDATE connection_request_authorizations SET started_message_id = ?1, started_event_digest = ?2, started_job_id = ?3, started_runner_id = ?4, started_runner_name = ?5 WHERE connection_id = ?6 AND scale_set_id = ?7 AND request_id = ?8 AND acquire_state = 'acquired' AND started_event_digest IS NULL AND (completed_event_digest IS NULL OR (completed_runner_id = ?4 AND completed_runner_name = ?5 AND completed_intent_id = ?9))",
        (record.message_id, record.event_digest.as_str(), record.job_id, record.runner.id, record.runner.name.as_str(), record.connection_id.as_str(), set_id, record.request_id, intent_id),
    ).await.map_err(|_| HostError::Journal)?;
    require_one_change(changed)
}

async fn record_started_jit(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    set_id: i64,
    intent_id: i64,
) -> Result<(), HostError> {
    let changed = connection.execute(
        "UPDATE connection_runner_jits SET started_request_id = ?1, state = CASE WHEN state = 'completed' THEN state ELSE 'started' END WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND runner_id = ?5 AND runner_name = ?6 AND state IN ('ready', 'started', 'completed') AND (started_request_id IS NULL OR started_request_id = ?1) AND (completed_request_id IS NULL OR completed_request_id = ?1)",
        (record.request_id, intent_id, record.connection_id.as_str(), set_id, record.runner.id, record.runner.name.as_str()),
    ).await.map_err(|_| HostError::Journal)?;
    require_one_change(changed)
}

async fn record_started_intent(
    connection: &turso::Connection,
    record: &StartedRecord<'_>,
    set_id: i64,
    intent_id: i64,
) -> Result<(), HostError> {
    let changed = connection.execute(
        "UPDATE connection_intents SET runner_request_id = COALESCE(runner_request_id, ?1), launch_phase = CASE WHEN runner_completed = 1 THEN launch_phase ELSE 'runner_started' END WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id IS NULL OR runner_request_id = ?1) AND runner_name = ?5 AND github_runner_id = ?6",
        (record.request_id, intent_id, record.connection_id.as_str(), set_id, record.runner.name.as_str(), record.runner.id.to_string()),
    ).await.map_err(|_| HostError::Journal)?;
    require_one_change(changed)
}

async fn record_completed(
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

fn require_one_change(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

mod storage;
use storage::{
    bind_completed_pair, exact_started_intent, insert_completion_cleanup, matching_runner_intent,
    request_completed_runnerless, request_event_state,
};

mod validate;
use validate::{
    validate_event, validate_job_id_history, validate_optional_event_text, validate_runner,
    validate_source,
};
