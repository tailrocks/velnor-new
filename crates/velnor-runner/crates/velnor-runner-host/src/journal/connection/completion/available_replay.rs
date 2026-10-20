//! Read-only proof that an Available message belongs to a durable terminal request.

use serde::{Deserialize, Serialize};
use velnor_runner_github::{InnerJob, InnerKind, SourceMetadata};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;
use crate::journal::source_authorization::EventSource;

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";

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

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum AcquireResponse {
    Returned { request_ids: Vec<i64> },
    NoEffect,
    AlreadyAcquired,
    Rejected,
    Uncertain,
}

impl ConnectionJournal {
    /// Retain an opaque binding proof without exposing the configured digest to launch code.
    pub(crate) fn terminal_replay_binding_token(&self) -> TerminalReplayBinding {
        TerminalReplayBinding(self.binding_digest().to_owned())
    }

    /// Compare an opaque receipt binding against this connection journal.
    pub(crate) fn terminal_replay_binding_matches(&self, expected: &TerminalReplayBinding) -> bool {
        self.binding_digest() == expected.0
    }

    /// Prove that every distinct request in this envelope already has a valid terminal event.
    pub(crate) async fn owns_terminal_available_requests(
        &self,
        scale_set_id: i64,
        entries: &[(i64, SourceMetadata, Option<String>)],
    ) -> Result<bool, HostError> {
        if scale_set_id <= 0 || entries.is_empty() {
            return Ok(false);
        }
        let mut unique = Vec::with_capacity(entries.len());
        for (request_id, source, job_id) in entries {
            let Some(source) = source.complete() else {
                return Ok(false);
            };
            if *request_id <= 0 {
                return Ok(false);
            }
            if let Some((_, existing_source, existing_job_id)) =
                unique.iter().find(|(id, _, _)| *id == *request_id)
            {
                if existing_source != &source || existing_job_id != job_id {
                    return Ok(false);
                }
            } else {
                unique.push((*request_id, source, job_id.clone()));
            }
        }

        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if !current_scale_set_matches(connection, &connection_id, scale_set_id).await? {
                return Ok(false);
            }
            for (request_id, source, job_id) in &unique {
                if !request_is_terminal(
                    connection,
                    &connection_id,
                    scale_set_id,
                    *request_id,
                    &binding_digest,
                    *source,
                    job_id.as_deref(),
                )
                .await?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })
        .await
    }

    /// Confirm each native lifecycle event in a mixed envelope already matches its durable record.
    pub(crate) async fn terminal_lifecycle_events_match(
        &self,
        scale_set_id: i64,
        message_id: i64,
        jobs: &[InnerJob],
    ) -> Result<bool, HostError> {
        if scale_set_id <= 0 || message_id < 0 {
            return Ok(false);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if !current_scale_set_matches(connection, &connection_id, scale_set_id).await? {
                return Ok(false);
            }
            for job in jobs {
                if !matches!(job.kind, InnerKind::Started | InnerKind::Completed) {
                    continue;
                }
                let Some(request_id) = job.request_id.filter(|id| *id > 0) else {
                    return Ok(false);
                };
                let Some(source) = job.source.complete() else {
                    return Ok(false);
                };
                if !request_is_terminal(
                    connection,
                    &connection_id,
                    scale_set_id,
                    request_id,
                    &binding_digest,
                    source,
                    job.job_id.as_deref(),
                )
                .await?
                    || !event_fields_match(
                        connection,
                        &connection_id,
                        scale_set_id,
                        request_id,
                        message_id,
                        job,
                    )
                    .await?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })
        .await
    }
}

pub(crate) struct TerminalReplayBinding(String);

impl std::fmt::Debug for TerminalReplayBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TerminalReplayBinding([redacted])")
    }
}

mod events;
mod request;
mod source;

use events::{current_scale_set_matches, event_fields_match};
use request::request_is_terminal;
