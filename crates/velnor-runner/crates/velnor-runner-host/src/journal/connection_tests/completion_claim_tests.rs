//! Test claim expiry and effect checks for completion cleanup in one repository connection.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::{
    NativeStartedIntakeFixture, helpers, native_started_intake_fixture,
};
use crate::journal::{ConnectionJournal, Journal};

async fn completed_fixture() -> Result<NativeStartedIntakeFixture, HostError> {
    let fixture = native_started_intake_fixture().await?;
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                &fixture.assignment,
                fixture.message_id,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("opaque-started-job"),
            )
            .await?,
        Some(fixture.intent_id)
    );
    Ok(fixture)
}

async fn reopened_scope(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(Journal, ConnectionJournal), HostError> {
    let journal = Journal::open(&fixture.database_path).await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let scope = journal.connection_scope(&binding).await?;
    Ok((journal, scope))
}

async fn set_claim_generation(
    fixture: &NativeStartedIntakeFixture,
    generation: i64,
) -> Result<(), HostError> {
    let path = fixture.database_path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_completion_cleanup SET claim_generation = ?1 WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4",
            (
                generation,
                fixture.intent_id,
                fixture.scope.connection_id().as_str(),
                501_i64,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[tokio::test]
async fn scoped_claim_check_rejects_expiry_and_generation_change() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let scope = &fixture.scope;
    let id = fixture.intent_id;
    let claim = scope
        .claim_completion_cleanup_at(id, 100, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        scope
            .completion_cleanup_claim_current_at(id, claim.generation, 189)
            .await?
    );
    assert!(
        !scope
            .completion_cleanup_claim_current_at(id, claim.generation, 190)
            .await?
    );
    let renewed = scope
        .claim_completion_cleanup_at(id, 190, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(renewed.generation > claim.generation);
    assert!(
        !scope
            .completion_cleanup_claim_current_at(id, claim.generation, 190)
            .await?
    );
    assert!(
        scope
            .completion_cleanup_claim_current_at(id, renewed.generation, 190)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn scoped_claim_retries_are_fenced_and_survive_reopen() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let id = fixture.intent_id;
    let scope = &fixture.scope;
    let first = scope
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first.generation, 1);
    assert_eq!(first.attempt, 1);
    assert_eq!(
        scope
            .due_completed_launches(100, 10)
            .await?
            .into_iter()
            .next(),
        None
    );
    assert!(
        scope
            .record_completion_runner_absent_at(id, first.generation, 109)
            .await?
    );
    assert!(
        scope
            .retry_completion_cleanup_at(id, first.generation, 109, 119)
            .await?
    );
    assert!(
        !scope
            .completion_cleanup_claim_current_at(id, first.generation, 109)
            .await?
    );
    assert_eq!(
        scope
            .due_completed_launches(118, 10)
            .await?
            .into_iter()
            .next(),
        None
    );
    let due = scope.due_completed_launches(119, 10).await?;
    assert_eq!(due.len(), 1);
    assert!(due[0].identity.runner_absent);

    let (_reopened_journal, reopened) = reopened_scope(&fixture).await?;
    let second = reopened
        .claim_completion_cleanup_at(id, 119, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(second.generation, 2);
    assert_eq!(second.attempt, 2);
    assert!(
        !reopened
            .retry_completion_cleanup_at(id, first.generation, 120, 120)
            .await?
    );
    assert!(
        reopened
            .completion_cleanup_claim_current_at(id, second.generation, 120)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn scoped_claim_generation_does_not_wrap_at_integer_max() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    set_claim_generation(&fixture, i64::MAX).await?;
    assert_eq!(
        fixture
            .scope
            .claim_completion_cleanup_at(fixture.intent_id, 10, 10)
            .await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn scoped_retry_backoff_caps_while_generation_keeps_increasing() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    for (index, ordinal) in (1..=7).enumerate() {
        let now = i64::try_from(index).map_err(|_| HostError::Journal)?;
        let claim = fixture
            .scope
            .claim_completion_cleanup_at(fixture.intent_id, now, 1)
            .await?
            .ok_or(HostError::Journal)?;
        assert_eq!(claim.generation, i64::from(ordinal));
        assert_eq!(claim.attempt, ordinal.min(5));
    }
    Ok(())
}

#[tokio::test]
async fn scoped_effect_renewal_blocks_reclaim_until_response_and_backoff() -> Result<(), HostError>
{
    let fixture = completed_fixture().await?;
    let id = fixture.intent_id;
    let first = fixture
        .scope
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let (_reopened_journal, other_scope) = reopened_scope(&fixture).await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_scope = fixture.scope.clone();
    let generation = first.generation;
    let effect = tokio::spawn(async move {
        running_scope
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;
    assert!(
        other_scope
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );
    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(effect.await.map_err(|_| HostError::Journal)??, Some(()));
    assert!(
        other_scope
            .retry_completion_cleanup_at(id, generation, 238, 240)
            .await?
    );
    assert!(
        other_scope
            .claim_completion_cleanup_at(id, 239, 10)
            .await?
            .is_none()
    );
    let next = other_scope
        .claim_completion_cleanup_at(id, 240, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(next.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn overlapping_scoped_effects_keep_the_longest_claim_deadline() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let id = fixture.intent_id;
    let second_scope = fixture.scope.clone();
    let first = fixture
        .scope
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_scope = fixture.scope.clone();
    let generation = first.generation;
    let first_effect = tokio::spawn(async move {
        running_scope
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;
    assert_eq!(
        second_scope
            .run_completion_cleanup_effect_at(id, generation, 110, 10, || async { Ok(()) })
            .await?,
        Some(())
    );
    assert!(
        second_scope
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );
    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(
        first_effect.await.map_err(|_| HostError::Journal)??,
        Some(())
    );
    let reclaimed = second_scope
        .claim_completion_cleanup_at(id, 239, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reclaimed.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn stale_expired_invalid_and_overflow_scoped_effects_do_not_run() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let scope = &fixture.scope;
    let id = fixture.intent_id;
    let claim = scope
        .claim_completion_cleanup_at(id, 10, 10)
        .await?
        .ok_or(HostError::Journal)?;
    let calls = AtomicUsize::new(0);
    let stale = scope
        .run_completion_cleanup_effect_at(id, claim.generation + 1, 11, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    let expired = scope
        .run_completion_cleanup_effect_at(id, claim.generation, 20, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    assert!(stale.is_none());
    assert!(expired.is_none());
    assert_eq!(
        scope
            .run_completion_cleanup_effect_at(id, claim.generation, 11, 0, || async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        scope
            .run_completion_cleanup_effect_at(id, claim.generation, i64::MAX - 1, 2, || async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },)
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn general_scoped_cleanup_cannot_release_completion_work() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    assert_eq!(
        fixture.scope.record_cleanup(fixture.intent_id).await,
        Err(HostError::Journal)
    );
    assert_eq!(fixture.scope.due_completed_launches(0, 10).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn scoped_cleanup_proof_is_claim_fenced_and_releases_its_row() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let scope = &fixture.scope;
    let id = fixture.intent_id;
    let claim = scope
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        !scope
            .record_completion_runner_absent_at(id, claim.generation + 1, 109)
            .await?
    );
    assert!(
        !scope
            .record_completion_runner_absent_at(id, claim.generation, 110)
            .await?
    );
    assert!(
        !scope
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        scope
            .record_completion_runner_absent_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        scope
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    assert_eq!(
        scope
            .due_completed_launches(109, 10)
            .await?
            .into_iter()
            .next(),
        None
    );
    Ok(())
}
