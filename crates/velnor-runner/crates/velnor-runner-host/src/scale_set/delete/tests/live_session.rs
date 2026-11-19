// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Fixture, ScaleSetDeleteOutcome, TestResult, attempt, delete_state, fixture, found, ok,
};
use crate::journal::{CapacityLimits, OperationalCapacityLimits};

async fn bind_session(fixture: &Fixture) -> TestResult<()> {
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 13 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 500,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    let nonce = "0123456789abcdef0123456789abcdef";
    fixture.scoped.prepare_session_create(nonce).await?;
    let claim = fixture
        .scoped
        .claim_session_create(nonce)
        .await?
        .ok_or("session claim")?;
    fixture
        .scoped
        .record_created_session(&claim, "session-1", 0)
        .await?;
    Ok(())
}

#[tokio::test]
async fn a_live_bound_session_does_not_hold_back_the_delete() -> TestResult<()> {
    let fixture = fixture(true).await?;
    bind_session(&fixture).await?;
    assert!(fixture.scoped.is_drained().await?);
    let (outcome, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(script.deletes(), 1);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_live_bound_session_plus_a_running_job_still_blocks_the_delete() -> TestResult<()> {
    let fixture = fixture(true).await?;
    bind_session(&fixture).await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id) VALUES ('conn-test-1', 'launch', 'runner-1', 'pending', 10)",
            (),
        )
        .await?;
    let (outcome, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::NotDrained);
    assert_eq!(script.requests(), 0);
    Ok(())
}
