//! Completion claim fencing, retry, renewal, and proof tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::reconcile::occupies;
use crate::{HostError, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-completion-claim-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.0);
        let _ignored = cleanup.err().map(|error| error.kind());
    }
}

async fn completed_launch(journal: &Journal) -> Result<i64, HostError> {
    let (id, _) = journal
        .begin_assigned_launch("m1r61", 77, 61, "v61")
        .await?;
    assert_eq!(
        journal.record_runner_completed(77, 61, 901, "v61").await?,
        Some(id)
    );
    Ok(id)
}

async fn set_claim_generation(
    path: &std::path::Path,
    id: i64,
    generation: i64,
) -> Result<(), HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE completion_cleanup SET claim_generation = ?1 WHERE intent_id = ?2",
            (generation, id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[tokio::test]
async fn claims_fence_retries_and_survive_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new("retry")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first.generation, 1);
    assert_eq!(first.attempt, 1);
    assert!(journal.due_completed_launches(100, 10).await?.is_empty());
    assert!(
        journal
            .record_completion_runner_absent_at(id, first.generation, 109)
            .await?
    );
    assert!(
        journal
            .retry_completion_cleanup_at(id, first.generation, 109, 119)
            .await?
    );
    assert!(
        !journal
            .completion_cleanup_claim_current_at(id, first.generation, 109)
            .await?
    );
    assert!(journal.due_completed_launches(118, 10).await?.is_empty());
    let due = journal.due_completed_launches(119, 10).await?;
    assert_eq!(due.len(), 1);
    assert!(due[0].identity.runner_absent);
    let reopened = Journal::open(&scratch.file()).await?;
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
async fn claim_generation_never_reuses_or_wraps_at_integer_max() -> Result<(), HostError> {
    let scratch = Scratch::new("generation-overflow")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    set_claim_generation(&scratch.file(), id, i64::MAX).await?;
    assert_eq!(
        journal.claim_completion_cleanup_at(id, 10, 10).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn retry_attempt_backoff_caps_while_generation_keeps_increasing() -> Result<(), HostError> {
    let scratch = Scratch::new("attempt-cap")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    for (index, ordinal) in (1..=7).enumerate() {
        let now = i64::try_from(index).map_err(|_| HostError::Journal)?;
        let claim = journal
            .claim_completion_cleanup_at(id, now, 1)
            .await?
            .ok_or(HostError::Journal)?;
        assert_eq!(claim.generation, i64::from(ordinal));
        let expected_attempt = u32::try_from(ordinal.min(5)).map_err(|_| HostError::Journal)?;
        assert_eq!(claim.attempt, expected_attempt);
    }
    Ok(())
}

#[tokio::test]
async fn effect_renewal_blocks_reclaim_until_response_and_backoff() -> Result<(), HostError> {
    let scratch = Scratch::new("effect-renewal")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let second_handle = Journal::open(&scratch.file()).await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_journal = journal.clone();
    let generation = first.generation;
    let effect = tokio::spawn(async move {
        running_journal
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );
    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(effect.await.map_err(|_| HostError::Journal)??, Some(()));
    assert!(
        second_handle
            .retry_completion_cleanup_at(id, generation, 238, 240)
            .await?
    );
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 239, 10)
            .await?
            .is_none()
    );
    let next = second_handle
        .claim_completion_cleanup_at(id, 240, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(next.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn overlapping_effects_keep_the_longest_claim_deadline() -> Result<(), HostError> {
    let scratch = Scratch::new("overlapping-effects")?;
    let journal = Journal::open(&scratch.file()).await?;
    let second_handle = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_journal = journal.clone();
    let generation = first.generation;
    let first_effect = tokio::spawn(async move {
        running_journal
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;

    let shorter = second_handle
        .run_completion_cleanup_effect_at(id, generation, 110, 10, || async { Ok(()) })
        .await?;
    assert_eq!(shorter, Some(()));
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );

    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(
        first_effect.await.map_err(|_| HostError::Journal)??,
        Some(())
    );
    let reclaimed = second_handle
        .claim_completion_cleanup_at(id, 239, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reclaimed.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn stale_expired_invalid_and_overflow_effects_do_not_run() -> Result<(), HostError> {
    let scratch = Scratch::new("effect-rejection")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 10, 10)
        .await?
        .ok_or(HostError::Journal)?;
    let calls = AtomicUsize::new(0);
    let stale = journal
        .run_completion_cleanup_effect_at(id, claim.generation + 1, 11, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    let expired = journal
        .run_completion_cleanup_effect_at(id, claim.generation, 20, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    assert!(stale.is_none());
    assert!(expired.is_none());
    assert_eq!(
        journal
            .run_completion_cleanup_effect_at(id, claim.generation, 11, 0, || async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal
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
async fn final_proof_is_claim_fenced_and_releases_uncertain_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new("final-proof")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, _) = journal
        .begin_assigned_launch("m1r62", 77, 62, "v62")
        .await?;
    assert!(journal.claim_assigned_acquire(id).await?);
    journal.record_assigned_acquire(id, true).await?;
    journal.finish(id, Outcome::Uncertain).await?;
    assert_eq!(
        journal.record_runner_completed(77, 62, 902, "v62").await?,
        Some(id)
    );
    let before = journal.rows().await?.remove(0);
    assert!(occupies(&before));
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        journal
            .bind_completion_containers_at(
                id,
                claim.generation,
                Some("ctr-runner"),
                Some("ctr-dind"),
                101
            )
            .await?
    );
    assert!(
        !journal
            .bind_completion_containers_at(id, claim.generation, Some("ctr-other"), None, 102)
            .await?
    );
    assert!(
        !journal
            .record_completion_runner_absent_at(id, claim.generation + 1, 103)
            .await?
    );
    assert!(
        !journal
            .record_completion_runner_absent_at(id, claim.generation, 110)
            .await?
    );
    assert!(
        !journal
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        journal
            .record_completion_runner_absent_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        journal
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    let after = Journal::open(&scratch.file())
        .await?
        .rows()
        .await?
        .remove(0);
    assert!(after.cleanup_proven);
    assert!(!occupies(&after));
    assert_eq!(after.state, crate::IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn general_cleanup_cannot_release_a_completion_row() -> Result<(), HostError> {
    let scratch = Scratch::new("general-cleanup-fence")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let second_handle = Journal::open(&scratch.file()).await?;

    assert_eq!(
        second_handle.record_cleanup(id).await,
        Err(HostError::Journal)
    );
    let rows = second_handle.rows().await?;
    assert!(occupies(rows.first().ok_or(HostError::Journal)?));
    assert!(!rows[0].cleanup_proven);
    let due = second_handle.due_completed_launches(0, 10).await?;
    assert_eq!(due.len(), 1);
    assert!(!due[0].identity.runner_absent);
    Ok(())
}
