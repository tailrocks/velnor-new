//! Durable completion-event matching and slot release rules.

use crate::HostError;
use crate::journal::LaunchReservation;
use crate::launch_harness::open;

#[tokio::test]
async fn completion_is_durable_and_keeps_capacity_until_cleanup() -> Result<(), String> {
    let (scratch, journal) = open("completion-durable").await?;
    let id = launch(&journal, 81).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let name = format!("v{}", identity.launch_id());

    assert_eq!(
        journal.record_runner_completed(1, 81, 91, &name).await,
        Ok(Some(id))
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    let row = journal
        .intent(id)
        .await
        .map_err(|error| error.to_string())?;
    assert!(row.runner_completed);
    assert_eq!(row.github_runner_id.as_deref(), Some("91"));
    assert_eq!(
        journal
            .completed_launches()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );

    let reopened = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .completed_launches()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    reopened
        .record_cleanup(id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(reopened.occupied_launches().await, Ok(0));
    assert!(
        reopened
            .completed_launches()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    assert_eq!(
        reopened.reserve_assignment(1, 81, 999, 1).await,
        Ok(LaunchReservation::Completed(id))
    );
    assert_eq!(
        reopened
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn completion_rejects_request_and_runner_id_conflicts() -> Result<(), String> {
    let (_scratch, journal) = open("completion-conflict").await?;
    let id = launch(&journal, 82).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let name = format!("v{}", identity.launch_id());
    journal
        .bind_github_runner(id, "92")
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        journal.record_runner_completed(1, 83, 92, &name).await,
        Err(HostError::Ownership)
    );
    assert_eq!(
        journal.record_runner_completed(1, 82, 93, &name).await,
        Err(HostError::Ownership)
    );
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .runner_completed
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    Ok(())
}

#[tokio::test]
async fn unrelated_runner_completion_does_not_change_a_launch() -> Result<(), String> {
    let (_scratch, journal) = open("completion-unrelated").await?;
    let id = launch(&journal, 84).await?;
    assert_eq!(
        journal
            .record_runner_completed(1, 84, 94, "runner-from-another-host")
            .await,
        Ok(None)
    );
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .runner_completed
    );
    Ok(())
}

#[tokio::test]
async fn cleaned_failed_retry_can_complete_and_remain_idempotent() -> Result<(), String> {
    let (_scratch, journal) = open("completion-after-retry").await?;
    let first = launch(&journal, 85).await?;
    journal
        .finish(first, crate::Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup(first)
        .await
        .map_err(|error| error.to_string())?;

    let LaunchReservation::New(retry) = journal
        .reserve_assignment(1, 85, 186, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new attempt after proven cleanup".to_owned());
    };
    assert_eq!(
        journal
            .reserve_assignment(1, 85, 187, 1)
            .await
            .map_err(|error| error.to_string())?,
        LaunchReservation::Existing(retry)
    );
    if !journal
        .claim_acquire(retry)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the retry acquire claim".to_owned());
    }
    journal
        .resolve_acquire(retry, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(retry)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the retry JIT claim".to_owned());
    }
    let identity = journal
        .launch_identity(retry)
        .await
        .map_err(|error| error.to_string())?;
    let name = format!("v{}", identity.launch_id());
    assert_eq!(
        journal.record_runner_completed(1, 85, 95, &name).await,
        Ok(Some(retry))
    );
    journal
        .record_cleanup(retry)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_assignment(1, 85, 188, 1)
            .await
            .map_err(|error| error.to_string())?,
        LaunchReservation::Completed(retry)
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    Ok(())
}

#[path = "journal_completion_claim_tests.rs"]
mod claim_tests;

#[path = "journal_completion_effect_tests.rs"]
mod effect_tests;

#[path = "journal_completion_clock_tests.rs"]
mod clock_tests;

#[path = "journal_completion_lineage_tests.rs"]
mod lineage_tests;

async fn launch(journal: &crate::Journal, request_id: i64) -> Result<i64, String> {
    let reservation = journal
        .reserve_assignment(1, request_id, 100 + request_id, 1)
        .await
        .map_err(|error| error.to_string())?;
    let LaunchReservation::New(id) = reservation else {
        return Err("expected a new launch reservation".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the first acquire claim".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the first JIT claim".to_owned());
    }
    Ok(id)
}
