//! This module checks whether an exact poll has only terminal requests.

use sha2::{Digest, Sha256};
use velnor_runner_github::{InnerKind, Poll, Statistics};

use crate::error::HostError;
use crate::journal::{ConnectionJournal, TerminalReplayBinding};

type AvailableEntry = (i64, velnor_runner_github::SourceMetadata, Option<String>);

struct AvailableEntries {
    values: Vec<AvailableEntry>,
    request_ids: Vec<i64>,
}

/// This receipt permits an ACK for one exact poll after its requests reach a terminal state.
#[derive(Debug)]
pub(crate) struct OwnedTerminalReplay {
    message_id: i64,
    body_digest: String,
    parsed_digest: String,
    session_id: String,
    connection_id: String,
    binding: TerminalReplayBinding,
    scale_set_id: i64,
    request_ids: Vec<i64>,
    statistics: Option<Statistics>,
}

/// This function checks that every Available request in the batch is terminal in this scope.
pub(crate) async fn classify_owned_terminal_available(
    scoped: &ConnectionJournal,
    scale_set_id: i64,
    session_id: &str,
    polled: &Poll,
) -> Result<Option<OwnedTerminalReplay>, HostError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    if batch.message_id < 0 || scale_set_id <= 0 {
        return Ok(None);
    }
    let Some(mut available) = available_entries(batch) else {
        return Ok(None);
    };
    if !scoped
        .owns_terminal_available_requests(scale_set_id, &available.values)
        .await?
        || !scoped
            .terminal_lifecycle_events_match(scale_set_id, batch.message_id, &batch.jobs)
            .await?
    {
        return Ok(None);
    }
    available.request_ids.sort_unstable();
    Ok(Some(OwnedTerminalReplay {
        message_id: batch.message_id,
        body_digest: digest(batch.raw_body.as_bytes()),
        parsed_digest: digest(format!("{batch:?}").as_bytes()),
        session_id: session_id.to_owned(),
        connection_id: scoped.connection_id().as_str().to_owned(),
        binding: scoped.terminal_replay_binding_token(),
        scale_set_id,
        request_ids: available.request_ids,
        statistics: batch.statistics.clone(),
    }))
}

fn available_entries(batch: &velnor_runner_github::ParsedBatch) -> Option<AvailableEntries> {
    if batch.jobs.iter().any(|job| {
        !matches!(
            job.kind,
            InnerKind::Available | InnerKind::Assigned | InnerKind::Started | InnerKind::Completed
        )
    }) {
        return None;
    }
    let mut entries: Vec<AvailableEntry> = Vec::new();
    let mut request_ids = Vec::new();
    for job in &batch.jobs {
        if job.kind != InnerKind::Available {
            continue;
        }
        let request_id = job.request_id.filter(|id| *id > 0)?;
        if !request_ids.contains(&request_id) {
            request_ids.push(request_id);
            entries.push((request_id, job.source.clone(), job.job_id.clone()));
        } else if !entries.iter().any(|(id, source, job_id)| {
            *id == request_id && source == &job.source && job_id == &job.job_id
        }) {
            return None;
        }
    }
    (!entries.is_empty()).then_some(AvailableEntries {
        values: entries,
        request_ids,
    })
}

impl OwnedTerminalReplay {
    /// This function checks the receipt against the same poll, scope, set, and complete request-ID set.
    pub(crate) fn matches_poll(
        &self,
        polled: &Poll,
        scoped: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> bool {
        let Poll::Batch(batch) = polled else {
            return false;
        };
        let Some(mut available) = available_entries(batch) else {
            return false;
        };
        available.request_ids.sort_unstable();
        batch.message_id == self.message_id
            && digest(batch.raw_body.as_bytes()) == self.body_digest
            && digest(format!("{batch:?}").as_bytes()) == self.parsed_digest
            && session_id == self.session_id
            && scoped.connection_id().as_str() == self.connection_id
            && scoped.terminal_replay_binding_matches(&self.binding)
            && scale_set_id == self.scale_set_id
            && batch.statistics == self.statistics
            && available.request_ids == self.request_ids
    }
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}
