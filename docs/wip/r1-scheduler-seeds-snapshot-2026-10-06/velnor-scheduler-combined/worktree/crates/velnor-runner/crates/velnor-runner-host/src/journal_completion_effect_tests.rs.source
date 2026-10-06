//! Effect fencing after a completion claim expires during one effect.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::Journal;
use crate::launch_harness::open;

#[tokio::test]
async fn expiry_during_one_effect_blocks_the_next_effect() -> Result<(), String> {
    let (_scratch, journal) = open("completion-effect-expiry").await?;
    let id = super::launch(&journal, 155).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_completed(1, 155, 1_155, &format!("v{}", identity.launch_id()))
        .await
        .map_err(|error| error.to_string())?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion cleanup claim".to_owned())?;
    let now = Arc::new(AtomicI64::new(109));
    let first_clock = Arc::clone(&now);
    let effect_now = Arc::clone(&now);
    assert_eq!(
        journal
            .run_completion_cleanup_effect_with_clock_for_test(
                id,
                claim.generation,
                move || Ok(first_clock.load(Ordering::Acquire)),
                move || async move {
                    effect_now.store(110, Ordering::Release);
                    Ok(())
                },
            )
            .await,
        Ok(Some(()))
    );

    let invoked = Arc::new(AtomicBool::new(false));
    let effect_invoked = Arc::clone(&invoked);
    let second_clock = Arc::clone(&now);
    assert_eq!(
        journal
            .run_completion_cleanup_effect_with_clock_for_test(
                id,
                claim.generation,
                move || Ok(second_clock.load(Ordering::Acquire)),
                move || async move {
                    effect_invoked.store(true, Ordering::Release);
                    Ok(())
                },
            )
            .await,
        Ok(None)
    );
    assert!(!invoked.load(Ordering::Acquire));
    assert_eq!(
        journal
            .claim_completion_cleanup_at(id, 110, 120)
            .await
            .map_err(|error| error.to_string())?
            .map(|next| next.generation),
        Some(claim.generation + 1)
    );
    Ok(())
}

#[tokio::test]
async fn authorization_rechecks_expiry_after_journal_io() -> Result<(), String> {
    let (_scratch, journal) = open("completion-effect-authorization-clock").await?;
    let id = super::launch(&journal, 157).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_completed(1, 157, 1_157, &format!("v{}", identity.launch_id()))
        .await
        .map_err(|error| error.to_string())?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion cleanup claim".to_owned())?;
    let current_reads = Arc::new(AtomicUsize::new(0));
    let current_clock = Arc::clone(&current_reads);
    assert_eq!(
        journal
            .completion_cleanup_claim_current_with_clock_for_test(id, claim.generation, move || Ok(
                if current_clock.fetch_add(1, Ordering::AcqRel) == 0 {
                    109
                } else {
                    110
                }
            ),)
            .await,
        Ok(false)
    );
    assert_eq!(current_reads.load(Ordering::Acquire), 2);

    let effect_reads = Arc::new(AtomicUsize::new(0));
    let effect_clock = Arc::clone(&effect_reads);
    let invoked = Arc::new(AtomicBool::new(false));
    let effect_invoked = Arc::clone(&invoked);
    assert_eq!(
        journal
            .run_completion_cleanup_effect_with_clock_for_test(
                id,
                claim.generation,
                move || Ok(if effect_clock.fetch_add(1, Ordering::AcqRel) == 0 {
                    109
                } else {
                    110
                }),
                move || async move {
                    effect_invoked.store(true, Ordering::Release);
                    Ok(())
                },
            )
            .await,
        Ok(None)
    );
    assert_eq!(effect_reads.load(Ordering::Acquire), 2);
    assert!(!invoked.load(Ordering::Acquire));
    Ok(())
}

#[tokio::test]
async fn separately_opened_journals_fence_takeover_until_effect_returns() -> Result<(), String> {
    let (scratch, journal) = open("completion-effect-second-handle").await?;
    let second = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert!(journal.shares_process_state(&second));
    let id = super::launch(&journal, 156).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_completed(1, 156, 1_156, &format!("v{}", identity.launch_id()))
        .await
        .map_err(|error| error.to_string())?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected first cleanup claim".to_owned())?;

    let (effect_started_tx, effect_started_rx) = tokio::sync::oneshot::channel();
    let (effect_release_tx, effect_release_rx) = tokio::sync::oneshot::channel();
    let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let effect_count = Arc::clone(&effects);
    let first_journal = journal.clone();
    let first = tokio::spawn(async move {
        first_journal
            .run_completion_cleanup_effect_with_clock_for_test(
                id,
                claim.generation,
                || Ok(109),
                move || async move {
                    effect_count.fetch_add(1, Ordering::AcqRel);
                    let _sent = effect_started_tx.send(());
                    let _released = effect_release_rx.await;
                    Ok(())
                },
            )
            .await
    });
    effect_started_rx.await.map_err(|error| error.to_string())?;

    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE completion_cleanup SET lease_until = 0 WHERE intent_id = ?1",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;

    let (attempt_started_tx, attempt_started_rx) = tokio::sync::oneshot::channel();
    let mut second_attempt = tokio::spawn(async move {
        let _sent = attempt_started_tx.send(());
        second.claim_completion_cleanup_at(id, 110, 120).await
    });
    attempt_started_rx
        .await
        .map_err(|error| error.to_string())?;
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut second_attempt)
            .await
            .is_err(),
        "a second Journal handle must wait while the first effect owns its intent"
    );

    let _sent = effect_release_tx.send(());
    assert_eq!(
        first.await.map_err(|error| error.to_string())?,
        Ok(Some(()))
    );
    let takeover = tokio::time::timeout(Duration::from_secs(1), &mut second_attempt)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expired claim was not reclaimed".to_owned())?;
    assert_eq!(takeover.generation, 2);
    assert_eq!(effects.load(Ordering::Acquire), 1);
    Ok(())
}
