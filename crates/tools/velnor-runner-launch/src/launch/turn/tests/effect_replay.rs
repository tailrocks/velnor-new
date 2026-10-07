//! Redelivery must not dispatch a launch with an unresolved prior effect.

use super::super::start_turn;
use super::{ready, zero_assignment_session};
use crate::launch::docker_stub::DockerStub;
use crate::launch::harness::{Mode, Script, assigned_wait, open};
use velnor_runner_host::{EnsureError, IntentState};
use velnor_runner_journal::journal::{Journal, LaunchClaim, LaunchEffectState, Outcome};

#[tokio::test]
async fn failed_may_have_effect_replay_after_restart_has_no_external_dispatch() -> Result<(), String>
{
    let (scratch, journal) = open("failed-may-have-effect-replay").await?;
    let row = persist_ambiguous_failure(&journal).await?;
    drop(journal);

    let reopened = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert_persisted_effect(&reopened, row).await?;
    assert_replay_claims(&reopened, row).await?;
    assert_offer_replay_has_no_external_effects(&reopened, row).await
}

async fn persist_ambiguous_failure(journal: &Journal) -> Result<i64, String> {
    let (row, fresh) = journal
        .begin_launch("m950")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_launch_identity(row, Some(950), Some(7301), None, None, "g2_950")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(row)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    // A later generic rejection cannot prove the earlier request had no effect.
    journal
        .finish(row, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row)
}

async fn assert_persisted_effect(journal: &Journal, row: i64) -> Result<(), String> {
    let persisted = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|candidate| candidate.id == row)
        .ok_or_else(|| "ambiguous launch row disappeared".to_owned())?;
    assert_eq!(persisted.state, IntentState::Failed);
    assert_eq!(persisted.launch_effect, LaunchEffectState::MayHaveEffect);
    assert_eq!(persisted.message_id, Some(950));
    assert_eq!(persisted.runner_request_id, Some(7301));
    Ok(())
}

async fn assert_replay_claims(journal: &Journal, row: i64) -> Result<(), String> {
    assert_eq!(
        journal
            .begin_launch("m950")
            .await
            .map_err(|error| error.to_string())?,
        (row, false)
    );
    assert_eq!(
        journal
            .begin_launch_if_accepting("m950")
            .await
            .map_err(|error| error.to_string())?,
        LaunchClaim::Existing(row)
    );
    assert_eq!(velnor_runner_launch_slot::occupied(journal).await, Ok(1));
    Ok(())
}

async fn assert_offer_replay_has_no_external_effects(
    journal: &Journal,
    row: i64,
) -> Result<(), String> {
    let session = zero_assignment_session()?;
    let polled = assigned_wait(950, 1);
    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers = Vec::new();
    let result = start_turn(
        &mut script,
        &mut workers,
        ready(&session, &polled),
        journal,
        &docker.docker,
        2,
        false,
    )
    .await;
    docker.finish().await?;

    assert!(matches!(result, Err(EnsureError::Uncertain)));
    assert!(
        script.calls.is_empty(),
        "Acquire, JIT, and ack must not repeat"
    );
    assert!(workers.is_empty(), "redelivery must not provision a worker");
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].launch_effect, LaunchEffectState::MayHaveEffect);
    Ok(())
}
