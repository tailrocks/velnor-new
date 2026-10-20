//! Exact Started and Completed event correlation to charged physical runners.

use serde::Serialize;

use crate::error::HostError;
use crate::journal::connection::reservation::post_jit_retirement::{
    EventRetirementDisposition, EventRetirementStatus,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordedEventDisposition {
    Active { intent_id: Option<i64> },
    Historical { intent_id: i64 },
}

fn historical_intent_id(disposition: EventRetirementDisposition) -> i64 {
    match disposition.status {
        EventRetirementStatus::Claimed | EventRetirementStatus::Retired => disposition.intent_id,
    }
}

impl RecordedEventDisposition {
    pub(crate) fn active(self) -> bool {
        matches!(self, Self::Active { .. })
    }
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
        match self
            .record_scoped_job_started(identity, message_id, source, runner, job_id)
            .await?
        {
            RecordedEventDisposition::Active {
                intent_id: Some(intent_id),
            }
            | RecordedEventDisposition::Historical { intent_id } => Ok(intent_id),
            RecordedEventDisposition::Active { intent_id: None } => Err(HostError::Journal),
        }
    }

    pub(crate) async fn record_scoped_job_started(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        source: &SourceMetadata,
        runner: &RunnerReference,
        job_id: Option<&str>,
    ) -> Result<RecordedEventDisposition, HostError> {
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
        let disposition = self
            .record_scoped_job_completed(identity, message_id, source, result, runner, job_id)
            .await?;
        Ok(match disposition {
            RecordedEventDisposition::Active { intent_id } => intent_id,
            RecordedEventDisposition::Historical { intent_id } => Some(intent_id),
        })
    }

    pub(crate) async fn record_scoped_job_completed(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        source: &SourceMetadata,
        result: Option<&str>,
        runner: Option<&RunnerReference>,
        job_id: Option<&str>,
    ) -> Result<RecordedEventDisposition, HostError> {
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

fn require_one_change(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

mod completion;
use completion::record_completed;

mod started;
use started::record_started;

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
