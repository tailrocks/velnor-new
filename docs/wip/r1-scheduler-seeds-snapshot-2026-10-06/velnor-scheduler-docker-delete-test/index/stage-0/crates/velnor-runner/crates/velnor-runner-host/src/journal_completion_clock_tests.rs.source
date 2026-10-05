use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use super::*;

#[tokio::test]
async fn worker_proof_samples_time_after_the_write_lock() -> Result<(), String> {
    let (_scratch, journal) = open("completion-clock-after-lock").await?;
    let id = completed_launch(&journal, 113).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected first claim".to_owned())?;

    let clock = Arc::new(AtomicI64::new(109));
    let sampled = Arc::new(AtomicBool::new(false));
    let guard = journal.write_guard().await;
    let (started, ready) = tokio::sync::oneshot::channel();
    let first_journal = journal.clone();
    let first_clock = Arc::clone(&clock);
    let first_sampled = Arc::clone(&sampled);
    let first_task = tokio::spawn(async move {
        let _sent = started.send(());
        first_journal
            .mark_completion_worker_cleanup_proven_with_clock_for_test(
                id,
                first.generation,
                move || {
                    first_sampled.store(true, Ordering::Release);
                    Ok(first_clock.load(Ordering::Acquire))
                },
            )
            .await
    });
    ready.await.map_err(|error| error.to_string())?;
    clock.store(110, Ordering::Release);
    if sampled.load(Ordering::Acquire) {
        return Err("worker proof sampled time before obtaining the journal lock".to_owned());
    }
    drop(guard);
    assert_eq!(
        first_task
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?,
        false,
        "worker proof must reject an expired claim"
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    Ok(())
}

#[tokio::test]
async fn final_release_samples_time_after_the_write_lock() -> Result<(), String> {
    let (_scratch, journal) = open("completion-release-clock-after-lock").await?;
    let id = completed_launch(&journal, 114).await?;
    let current = journal
        .claim_completion_cleanup_at(id, 110, 120)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected renewed claim".to_owned())?;
    assert!(
        journal
            .mark_completion_worker_cleanup_proven_at(id, current.generation, 119)
            .await
            .map_err(|error| error.to_string())?
    );

    let release_clock = Arc::new(AtomicI64::new(119));
    let release_sampled = Arc::new(AtomicBool::new(false));
    let guard = journal.write_guard().await;
    let (started, ready) = tokio::sync::oneshot::channel();
    let release_journal = journal.clone();
    let clock_value = Arc::clone(&release_clock);
    let sampled_value = Arc::clone(&release_sampled);
    let release_task = tokio::spawn(async move {
        let _sent = started.send(());
        release_journal
            .record_completion_cleanup_with_clock_for_test(id, current.generation, move || {
                sampled_value.store(true, Ordering::Release);
                Ok(clock_value.load(Ordering::Acquire))
            })
            .await
    });
    ready.await.map_err(|error| error.to_string())?;
    release_clock.store(120, Ordering::Release);
    if release_sampled.load(Ordering::Acquire) {
        return Err("final release sampled time before obtaining the journal lock".to_owned());
    }
    drop(guard);
    assert_eq!(
        release_task
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?,
        false,
        "final release must reject an expired claim"
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    Ok(())
}

async fn completed_launch(journal: &crate::Journal, request_id: i64) -> Result<i64, String> {
    let id = launch(journal, request_id).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let runner_id = request_id + 100;
    let runner_name = format!("v{}", identity.launch_id());
    journal
        .record_runner_completed(1, request_id, runner_id, &runner_name)
        .await
        .map_err(|error| error.to_string())?;
    Ok(id)
}
