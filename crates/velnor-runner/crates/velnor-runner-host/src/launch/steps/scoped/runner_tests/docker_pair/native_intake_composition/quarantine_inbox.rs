//! Check that the completion inbox stays unchanged after Started replay.

use super::{Fixture, Poll, SET_ID};

pub(super) async fn verify_quarantined_inbox(
    fixture: &Fixture,
    polled: &Poll,
    backlog_rows: usize,
) -> Result<(), String> {
    let Poll::Batch(batch) = polled else {
        return Err("The poll result is not a parsed batch.".to_owned());
    };
    let reopened = crate::Journal::open(&fixture.journal_file())
        .await
        .map_err(|error| format!("The completion inbox could not open again: {error}."))?;
    let scope = reopened
        .connection_scope(&fixture.binding)
        .await
        .map_err(|error| error.to_string())?;
    for message_id in 0..backlog_rows {
        let message_id =
            i64::try_from(message_id).map_err(|_| "The inbox ID is too large.".to_owned())?;
        if scope
            .store_completion_inbox(message_id, "replaced")
            .await
            .is_ok()
        {
            return Err(format!(
                "Completion inbox record {message_id} changed after the journal opened again."
            ));
        }
    }
    verify_mixed_body(&scope, batch.message_id, &batch.raw_body).await?;
    let replay = crate::launch::test_scoped_intake(&scope, SET_ID, polled)
        .await
        .map_err(|error| format!("Replaying the native mixed poll failed: {error}"))?;
    if replay.completion_only || !replay.wake_cleanup || !replay.started_request_ids.is_empty() {
        return Err("The intake result changed after the mixed poll replay.".to_owned());
    }
    verify_mixed_body(&scope, batch.message_id, &batch.raw_body).await
}

async fn verify_mixed_body(
    scope: &crate::journal::ConnectionJournal,
    message_id: i64,
    raw_body: &str,
) -> Result<(), String> {
    if scope
        .store_completion_inbox(message_id, "replaced")
        .await
        .is_ok()
    {
        return Err("The saved poll body changed after the journal opened again.".to_owned());
    }
    scope
        .store_completion_inbox(message_id, raw_body)
        .await
        .map_err(|error| format!("The same poll body could not be saved again: {error}."))
}
