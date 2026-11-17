// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "tests/fixture.rs"]
mod fixture;
#[path = "tests/restore_timeout.rs"]
mod restore_timeout;

use super::{ConnectionPollContext, FairnessTurn, PollTurnResult, poll_connection};
use fixture::Fixture;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test(start_paused = true)]
async fn a_failed_restore_holds_the_next_poll_after_the_session_is_bound() -> TestResult<()> {
    let mut fixture = Fixture::new().await?;
    let first = fixture.poll().await?;
    let now = tokio::time::Instant::now();
    let retry_at = fixture.prepared.restore_retry_at;

    fixture.bind_session().await?;
    fixture
        .prepared
        .scope
        .session_header_allocation(fixture.prepared.session_intent)
        .await?;
    fixture.control.record_session_poll_observation(
        &fixture.prepared.binding,
        fixture.prepared.set_id,
        fixture.prepared.session_intent,
        "session-test",
        None,
    )?;
    assert!(fixture.control.complete_session_poll_receipts()?.is_some());

    let next = fixture.poll().await?;

    assert!(
        next.is_none(),
        "credential restore backoff must hold the next poll"
    );
    assert!(first.is_none());
    assert_eq!(tokio::time::Instant::now(), now);
    assert!(retry_at.is_some());
    assert_eq!(fixture.prepared.restore_retry_at, retry_at);
    assert!(fixture.prepared.credentials.is_none());
    assert!(fixture.control.complete_session_poll_receipts()?.is_none());
    Ok(())
}
