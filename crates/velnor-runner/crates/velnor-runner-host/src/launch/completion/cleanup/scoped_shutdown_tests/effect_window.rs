use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::error::HostError;
use crate::journal::{Journal, ReservationState};
use crate::launch::completion::cleanup::{
    EffectBudget, Failure, cleanup_retry_delay_seconds, completion_now, run_effect,
};
use crate::launch::completion::journal_ref::CompletionJournal;

use super::super::{EFFECT_WINDOW_SECONDS, MAX_NATIVE_CLEANUP_EFFECTS};
use super::{CLAIM_LEASE_SECONDS, HttpsTransport, completed_fixture};

#[test]
fn native_budget_and_effect_window_cover_current_cleanup_bounds() -> Result<(), HostError> {
    let expected_effects = 3
        + 2 * 4 * 2
        + u64::try_from(crate::worker::WorkerVolumeRole::ALL.len())
            .map_err(|_| HostError::Journal)?
            * 3
            * 2
        + 7
        + 80;
    assert_eq!(MAX_NATIVE_CLEANUP_EFFECTS, expected_effects);
    assert_eq!(CLAIM_LEASE_SECONDS, EFFECT_WINDOW_SECONDS);
    let window = u64::try_from(EFFECT_WINDOW_SECONDS).map_err(|_| HostError::Journal)?;
    let curl = HttpsTransport::new("https://github.com")?.timeout_seconds();
    assert!(window >= curl + 30);
    assert!(window >= crate::docker_client::DOCKER_OPERATION_TIMEOUT.as_secs() + 30);
    assert_eq!(cleanup_retry_delay_seconds(1), EFFECT_WINDOW_SECONDS + 5);
    assert_eq!(cleanup_retry_delay_seconds(5), EFFECT_WINDOW_SECONDS + 80);
    Ok(())
}

#[test]
fn exhausted_budget_denies_effect_and_keeps_completion_charged() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "exhausted_budget_denies_effect_and_keeps_completion_charged",
        || {
            Box::pin(async {
                exhausted_budget_denies_effect_and_keeps_completion_charged_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn exhausted_budget_denies_effect_and_keeps_completion_charged_inner() -> Result<(), HostError>
{
    let fixture = completed_fixture().await?;
    let now = completion_now()?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(fixture.intent_id, now, CLAIM_LEASE_SECONDS)
        .await?
        .ok_or(HostError::Journal)?;
    let completion = CompletionJournal::scoped(fixture.scope.clone());
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    for _ in 0..MAX_NATIVE_CLEANUP_EFFECTS {
        assert!(budget.reserve("budget fixture").is_ok());
    }
    let effects = AtomicUsize::new(0);
    let failure = run_effect(
        &completion,
        &mut budget,
        fixture.intent_id,
        claim,
        "extra request",
        || async {
            effects.fetch_add(1, Ordering::Relaxed);
            Ok::<(), HostError>(())
        },
    )
    .await
    .err()
    .ok_or(HostError::Journal)?;
    assert_eq!(failure, Failure::bounded("extra request"));
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    assert!(
        fixture
            .scope
            .completion_cleanup_claim_current_at(fixture.intent_id, claim.generation, now + 1)
            .await?
    );
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        ReservationState::Reserved
    );
    assert!(
        !fixture
            .scope
            .intent(fixture.intent_id)
            .await?
            .cleanup_proven
    );
    Ok(())
}

#[test]
fn delayed_effect_renews_lease_and_keeps_retry_fenced() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "delayed_effect_renews_lease_and_keeps_retry_fenced",
        || {
            Box::pin(async {
                delayed_effect_renews_lease_and_keeps_retry_fenced_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn delayed_effect_renews_lease_and_keeps_retry_fenced_inner() -> Result<(), HostError> {
    const ORIGINAL_LEASE_SECONDS: i64 = 30;

    let fixture = completed_fixture().await?;
    let start = completion_now()?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(fixture.intent_id, start, ORIGINAL_LEASE_SECONDS)
        .await?
        .ok_or(HostError::Journal)?;
    let attempt = claim.attempt;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let competitor_journal = Journal::open(&fixture.database_path).await?;
    let competitor = competitor_journal.connection_scope(&binding).await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_scope = fixture.scope.clone();
    let intent_id = fixture.intent_id;
    let generation = claim.generation;
    let effect = spawn_delayed_effect(
        CompletionJournal::scoped(running_scope),
        intent_id,
        claim,
        started_tx,
        finish_rx,
    );
    started_rx.await.map_err(|_| HostError::Journal)?;
    let probe_at = completion_now()?
        .checked_add(ORIGINAL_LEASE_SECONDS + 1)
        .ok_or(HostError::Journal)?;
    assert!(
        competitor
            .claim_completion_cleanup_at(fixture.intent_id, probe_at, 10)
            .await?
            .is_none()
    );
    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    let failure = effect
        .await
        .map_err(|_| HostError::Journal)?
        .err()
        .ok_or(HostError::Journal)?;
    assert_eq!(
        failure,
        Failure::request("slow Docker request", HostError::Docker)
    );
    let retry_at = probe_at
        .checked_add(cleanup_retry_delay_seconds(attempt))
        .ok_or(HostError::Journal)?;
    assert!(
        competitor
            .retry_completion_cleanup_at(fixture.intent_id, generation, probe_at, retry_at)
            .await?
    );
    assert!(
        competitor
            .claim_completion_cleanup_at(fixture.intent_id, retry_at - 1, 10)
            .await?
            .is_none()
    );
    assert!(
        competitor
            .claim_completion_cleanup_at(fixture.intent_id, retry_at, 10)
            .await?
            .is_some()
    );
    Ok(())
}

fn spawn_delayed_effect(
    completion: CompletionJournal,
    intent_id: i64,
    claim: crate::journal::CleanupClaim,
    started_tx: tokio::sync::oneshot::Sender<()>,
    finish_rx: tokio::sync::oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<Result<(), Failure>> {
    tokio::spawn(async move {
        let stopping = AtomicBool::new(false);
        let mut budget = EffectBudget::new(&stopping);
        run_effect(
            &completion,
            &mut budget,
            intent_id,
            claim,
            "slow Docker request",
            || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Err::<(), HostError>(HostError::Docker)
            },
        )
        .await
    })
}
