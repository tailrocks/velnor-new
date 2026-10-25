use std::time::Instant;

use super::poll_fixture;
use crate::{
    journal::{ConnectionJournal, HeaderAllocation},
    launch::{FreshPressureSample, PressureSample},
};

fn low_pressure_sample() -> FreshPressureSample {
    FreshPressureSample::test_new(
        PressureSample {
            load_milli: 500,
            cpus: 8,
            memory_available_bytes: 8 * 1024 * 1024 * 1024,
            docker_root_free_bytes: Some(20 * 1024 * 1024 * 1024),
        },
        Instant::now(),
    )
}

async fn complete_pressure_poll(
    scope: &ConnectionJournal,
    intent_id: i64,
    allocation: &HeaderAllocation,
    session_id: &str,
    expected_header: u32,
) -> Result<(), String> {
    let attempt = scope
        .begin_session_pressure_poll(intent_id, allocation, Some(low_pressure_sample()))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(attempt.header_value(), expected_header);
    let proof = crate::launch::CompletedHeaderPoll::after_response(
        attempt, allocation, scope, 10_101, session_id,
    )
    .map_err(|error| error.to_string())?;
    scope
        .settle_session_poll(intent_id, proof)
        .await
        .map_err(|error| error.to_string())
}

async fn assert_full_charge(journal: &crate::journal::Journal) -> Result<(), String> {
    assert_eq!(
        journal
            .charged_header_capacity()
            .await
            .map_err(|error| error.to_string())?,
        4
    );
    Ok(())
}

#[tokio::test]
async fn pressure_completion_proof_settles_and_reopens_below_reserved_ceiling() -> Result<(), String>
{
    let fixture = poll_fixture().await?;
    let session_id = fixture.session.session_id.clone();
    complete_pressure_poll(
        &fixture.scope,
        fixture.intent_id,
        &fixture.allocation,
        &session_id,
        1,
    )
    .await?;
    assert_full_charge(&fixture.journal).await?;

    let super::PollFixture {
        _scratch: scratch,
        journal,
        scope,
        intent_id,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let journal = crate::journal::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let binding = crate::launch::turn::scoped_poll_tests::support::repository_connection()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    let allocation = scope
        .session_header_allocation(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    complete_pressure_poll(&scope, intent_id, &allocation, &session_id, 2).await?;
    assert_full_charge(&journal).await?;
    Ok(())
}
