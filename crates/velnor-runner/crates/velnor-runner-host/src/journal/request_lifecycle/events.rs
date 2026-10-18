//! Exact Started and Completed event correlation to charged physical runners.

use serde::Serialize;

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use velnor_runner_github::RunnerReference;

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
}

#[derive(Serialize)]
struct CompletionEvent<'a> {
    request_id: i64,
    message_id: i64,
    result: Option<&'a str>,
    runner: Option<RunnerEvent<'a>>,
    job_id: Option<&'a str>,
}

impl ConnectionJournal {
    /// Correlate an actual JobStarted request with its exact JIT-created runner.
    pub(crate) async fn record_job_started(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        runner: &RunnerReference,
        job_id: Option<&str>,
    ) -> Result<i64, HostError> {
        let request_id = request_id(identity, self)?;
        validate_event(message_id, runner, job_id)?;
        let event_digest = digest(
            &serde_json::to_vec(&RunnerEvent {
                request_id,
                message_id,
                runner_id: runner.id,
                runner_name: &runner.name,
                scale_set_id: runner.runner_scale_set_id,
                job_id,
            })
            .map_err(|_| HostError::Journal)?,
        );
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            let set_id = bound_set_id(connection, &connection_id).await?;
            if set_id != runner.runner_scale_set_id {
                return Err(HostError::Identity);
            }
            let Some(event_state) = request_event_state(
                connection,
                &connection_id,
                set_id,
                request_id,
            )
            .await?
            else {
                return Err(HostError::Journal);
            };
            if let Some(existing) = event_state.started_digest.as_deref() {
                if existing == event_digest {
                    return exact_started_intent(
                        connection,
                        &connection_id,
                        set_id,
                        request_id,
                        runner,
                    )
                    .await;
                }
                return Err(HostError::Identity);
            }
            if event_state.acquire_state != "acquired"
                || request_completed_runnerless(
                    connection,
                    &connection_id,
                    set_id,
                    request_id,
                )
                .await?
            {
                return Err(HostError::Journal);
            }
            let intent_id = matching_runner_intent(
                connection,
                &connection_id,
                set_id,
                runner.id,
                &runner.name,
                request_id,
                true,
            )
            .await?;
            let changed = connection
                .execute(
                    "UPDATE connection_request_authorizations SET started_message_id = ?1, started_event_digest = ?2, started_job_id = ?3, started_runner_id = ?4, started_runner_name = ?5 WHERE connection_id = ?6 AND scale_set_id = ?7 AND request_id = ?8 AND acquire_state = 'acquired' AND started_event_digest IS NULL AND (completed_event_digest IS NULL OR (completed_runner_id = ?4 AND completed_runner_name = ?5 AND completed_intent_id = ?9))",
                    (message_id, event_digest.as_str(), job_id, runner.id, runner.name.as_str(), connection_id.as_str(), set_id, request_id, intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET started_request_id = ?1, state = CASE WHEN state = 'completed' THEN state ELSE 'started' END WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND runner_id = ?5 AND runner_name = ?6 AND state IN ('ready', 'started', 'completed') AND (started_request_id IS NULL OR started_request_id = ?1) AND (completed_request_id IS NULL OR completed_request_id = ?1)",
                    (request_id, intent_id, connection_id.as_str(), set_id, runner.id, runner.name.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET runner_request_id = COALESCE(runner_request_id, ?1), launch_phase = CASE WHEN runner_completed = 1 THEN launch_phase ELSE 'runner_started' END WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id IS NULL OR runner_request_id = ?1) AND runner_name = ?5 AND github_runner_id = ?6",
                    (request_id, intent_id, connection_id.as_str(), set_id, runner.name.as_str(), runner.id.to_string()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(intent_id)
        })
        .await
    }

    /// Persist a request completion; only an exact runner reference links a pair.
    pub(crate) async fn record_job_completed(
        &self,
        identity: &AssignmentIdentity,
        message_id: i64,
        result: Option<&str>,
        runner: Option<&RunnerReference>,
        job_id: Option<&str>,
    ) -> Result<Option<i64>, HostError> {
        let request_id = request_id(identity, self)?;
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
            }),
            job_id,
        };
        let event_digest = digest(&serde_json::to_vec(&event).map_err(|_| HostError::Journal)?);
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            let set_id = bound_set_id(connection, &connection_id).await?;
            let Some(event_state) = request_event_state(
                connection,
                &connection_id,
                set_id,
                request_id,
            )
            .await?
            else {
                return Err(HostError::Journal);
            };
            if let Some(existing) = event_state.completed_digest.as_deref() {
                if existing == event_digest {
                    return Ok(event_state.intent_id);
                }
                return Err(HostError::Identity);
            }
            let completed_pair = if let Some(runner) = runner {
                if runner.runner_scale_set_id != set_id {
                    return Err(HostError::Identity);
                }
                let intent_id = matching_runner_intent(
                    connection,
                    &connection_id,
                    set_id,
                    runner.id,
                    &runner.name,
                    request_id,
                    false,
                )
                .await?;
                Some((intent_id, runner.id, runner.name.as_str()))
            } else {
                None
            };
            if let Some((intent_id, runner_id, runner_name)) = completed_pair {
                bind_completed_pair(
                    connection,
                    &connection_id,
                    set_id,
                    request_id,
                    intent_id,
                    runner_id,
                    runner_name,
                )
                .await?;
            }
            let (runner_id, runner_name, completed_intent) = completed_pair
                .map(|(intent_id, runner_id, runner_name)| {
                    (Some(runner_id), Some(runner_name), Some(intent_id))
                })
                .unwrap_or((None, None, None));
            let changed = connection
                .execute(
                    "UPDATE connection_request_authorizations SET completed_message_id = ?1, completed_event_digest = ?2, completed_job_id = ?3, completed_result = ?4, completed_runner_id = ?5, completed_runner_name = ?6, completed_intent_id = ?7 WHERE connection_id = ?8 AND scale_set_id = ?9 AND request_id = ?10 AND completed_event_digest IS NULL AND (started_event_digest IS NULL OR ?5 IS NULL OR (started_runner_id = ?5 AND started_runner_name = ?6))",
                    (message_id, event_digest.as_str(), job_id, result, runner_id, runner_name, completed_intent, connection_id.as_str(), set_id, request_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Identity);
            }
            if let Some((intent_id, runner_id, runner_name)) = completed_pair {
                let changed = connection
                    .execute(
                        "UPDATE connection_runner_jits SET completed_request_id = ?1, state = 'completed' WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND runner_id = ?5 AND runner_name = ?6 AND state IN ('bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed') AND (completed_request_id IS NULL OR completed_request_id = ?1) AND (started_request_id IS NULL OR started_request_id = ?1)",
                        (request_id, intent_id, connection_id.as_str(), set_id, runner_id, runner_name),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if changed != 1 {
                    return Err(HostError::Journal);
                }
                insert_completion_cleanup(
                    connection,
                    &connection_id,
                    set_id,
                    request_id,
                    intent_id,
                    runner_id,
                    runner_name,
                )
                .await?;
            }
            Ok(completed_pair.map(|(intent_id, _, _)| intent_id))
        })
        .await
    }
}

mod storage;
use storage::{
    bind_completed_pair, exact_started_intent, insert_completion_cleanup, matching_runner_intent,
    request_completed_runnerless, request_event_state,
};

fn validate_event(
    message_id: i64,
    runner: &RunnerReference,
    job_id: Option<&str>,
) -> Result<(), HostError> {
    if message_id < 0 {
        return Err(HostError::Identity);
    }
    validate_runner(runner)?;
    validate_optional_event_text(job_id)
}

fn validate_runner(runner: &RunnerReference) -> Result<(), HostError> {
    if runner.id <= 0
        || runner.runner_scale_set_id <= 0
        || runner.name.is_empty()
        || runner.name.len() > 128
        || runner.name.chars().any(char::is_control)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_optional_event_text(value: Option<&str>) -> Result<(), HostError> {
    if value.is_some_and(|text| text.len() > MAX_EVENT_TEXT || text.chars().any(char::is_control)) {
        return Err(HostError::Identity);
    }
    Ok(())
}
