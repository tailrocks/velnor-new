use std::sync::{Arc, Mutex};

use super::{Scratch, Seen, dir_contains, observed_state, open};
use crate::{HostError, IntentState, Outcome};

#[tokio::test]
async fn reopen_sees_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("reopen")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        journal.begin("provision", "job-1").await?
    };
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn uncertain_outcome_keeps_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("uncertain")?;
    let journal = open(&scratch.file()).await?;
    let held = journal.begin("acquire", "job-1").await?;
    journal.finish(held, Outcome::Uncertain).await?;
    let next = journal.begin("provision", "job-2").await?;
    assert_eq!(journal.read(held).await?, IntentState::Uncertain);
    assert_eq!(journal.read(next).await?, IntentState::Pending);
    drop(journal);
    let again = open(&scratch.file()).await?;
    assert_eq!(again.read(held).await?, IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn missing_row_is_not_a_successful_finish() -> Result<(), HostError> {
    let scratch = Scratch::new("missing")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(
        journal.finish(99, Outcome::Done).await,
        Err(HostError::Journal)
    );
    let kept = journal.begin("provision", "keep").await?;
    let id = journal.begin("acquire", "drop-me").await?;
    journal.finish(id, Outcome::DefiniteFailure).await?;
    assert_eq!(journal.read(id).await?, IntentState::Failed);
    assert_eq!(journal.read(kept).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn empty_and_quoted_kinds_are_rejected() -> Result<(), HostError> {
    let scratch = Scratch::new("kind")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(journal.begin("", "job").await, Err(HostError::Journal));
    assert_eq!(journal.begin("it's", "job").await, Err(HostError::Journal));
    assert_eq!(
        journal.begin("say \"hi\"", "job").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.begin("provision", "").await,
        Err(HostError::Journal)
    );
    let id = journal.begin("provision", "job-1").await?;
    assert_eq!(journal.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn drop_does_not_delete_pending_rows() -> Result<(), HostError> {
    let scratch = Scratch::new("drop")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        let id = journal.begin("provision", "job-1").await?;
        let clone = journal.clone();
        drop(clone);
        drop(journal);
        id
    };
    assert!(path.is_file());
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn around_commits_pending_before_effect_and_hides_secret() -> Result<(), HostError> {
    const CANARY: &str = "velnor-canary-jit-pat-admin";
    let scratch = Scratch::new("around")?;
    let path = scratch.file();
    let journal = open(&path).await?;
    let seen = Arc::new(Mutex::new(Seen {
        state: None,
        secret_ok: false,
    }));
    let slot = Arc::clone(&seen);
    let probe = path.clone();
    let id = journal
        .around("acquire", "job-1", CANARY, async move |secret| {
            let secret_ok = secret == CANARY;
            let state = observed_state(&probe).await;
            if let Ok(mut guard) = slot.lock() {
                guard.secret_ok = secret_ok;
                guard.state = state;
            }
            Outcome::Uncertain
        })
        .await?;
    {
        let guard = seen.lock().map_err(|_| HostError::Journal)?;
        assert!(guard.secret_ok);
        assert_eq!(guard.state, Some(IntentState::Pending));
    }
    assert_eq!(journal.read(id).await?, IntentState::Uncertain);
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Uncertain);
    assert!(!dir_contains(&scratch.path, CANARY)?);
    journal
        .bind(id, Some("ctr-bind-1"), Some("gh-bind-9"))
        .await?;
    assert!(dir_contains(&scratch.path, "ctr-bind-1")?);
    Ok(())
}

#[tokio::test]
async fn replay_reuses_live_subject_and_failed_starts_again() -> Result<(), HostError> {
    let scratch = Scratch::new("subject")?;
    let journal = open(&scratch.file()).await?;
    let first = journal.begin("acquire", "req-7").await?;
    assert_eq!(journal.begin("acquire", "req-7").await?, first);
    journal.finish(first, Outcome::Done).await?;
    assert_eq!(journal.begin("acquire", "req-7").await?, first);
    journal.finish(first, Outcome::DefiniteFailure).await?;
    let third = journal.begin("acquire", "req-7").await?;
    assert_ne!(third, first);
    assert_eq!(journal.rows().await?.len(), 2);
    let other = journal.begin("provision", "req-7").await?;
    assert_ne!(other, third);
    let held = journal.begin("acquire", "req-9").await?;
    journal.finish(held, Outcome::Uncertain).await?;
    assert_eq!(journal.begin("acquire", "req-9").await?, held);
    Ok(())
}

#[tokio::test]
async fn cleanup_proof_reopens() -> Result<(), HostError> {
    let scratch = Scratch::new("proof")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        let id = journal.begin("delete", "ctr-1").await?;
        journal.finish(id, Outcome::Done).await?;
        journal.bind(id, Some("ctr-1"), Some("gh-1")).await?;
        journal.record_cleanup(id).await?;
        id
    };
    let rows = open(&path).await?.rows().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert!(rows[0].cleanup_proven);
    assert_eq!(rows[0].docker_id.as_deref(), Some("ctr-1"));
    assert_eq!(rows[0].github_runner_id.as_deref(), Some("gh-1"));
    Ok(())
}
