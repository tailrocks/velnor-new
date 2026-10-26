// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::Poll;

use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::{Journal, Outcome};
use crate::scale_set::EnsureError;

use super::steps::Idle;

#[must_use]
pub(crate) fn should_ack(idle: Idle) -> bool {
    matches!(idle, Idle::Scale)
}

pub(crate) async fn fail_unstarted(journal: &Journal, polled: &Poll) -> Result<bool, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(false);
    };
    let subject = format!("m{}", batch.message_id);
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut matched = false;
    for row in &rows {
        if row.kind != "launch" || row.subject != subject {
            continue;
        }
        if row.docker_id.is_some() || row.dind_id.is_some() {
            return Ok(false);
        }
        matched = true;
    }
    if !matched {
        return Ok(false);
    }
    for row in rows {
        if row.kind != "launch" || row.subject != subject || row.state != IntentState::Uncertain {
            continue;
        }
        journal
            .finish(row.id, Outcome::DefiniteFailure)
            .await
            .map_err(map_journal)?;
    }
    Ok(true)
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
