//! Regressions for reopening and adopting rows from the pre-lineage journal.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_github::Poll;

use crate::daemon_lock::test_engine_lineage_guard;
use crate::journal::LaunchReservation;
use crate::launch::drive_offer;
use crate::launch_harness::{Mode, Script, available, ctx, prepare};
use crate::{EnsureError, HostError, IntentState, Journal};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-journal-migration-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.path);
    }
}

async fn create_old_schema_row(
    path: &Path,
    subject: &str,
    state: &str,
    cleanup_proven: bool,
) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO intents (id, kind, subject, state, docker_id, github_runner_id, cleanup_proven) VALUES (41, 'launch', ?1, ?2, 'legacy-runner-container', 'legacy-github-runner', ?3)",
            (subject, state, cleanup_proven),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn assignment_poll(message_id: i64, request_id: i64) -> Poll {
    let mut poll = available(&[request_id]);
    if let Poll::Batch(batch) = &mut poll {
        batch.message_id = message_id;
    }
    poll
}

#[tokio::test]
async fn uncertain_legacy_assignment_is_not_replayed_after_adoption_and_reopen()
-> Result<(), HostError> {
    let scratch = Scratch::new("uncertain")?;
    let path = scratch.file();
    create_old_schema_row(&path, "m100r42", "uncertain", false).await?;

    let journal = Journal::open(&path).await?;
    journal.bind_engine("docker-engine-migration-test").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    assert_eq!(
        drive_offer(
            &mut first,
            &ctx(),
            &assignment_poll(100, 42),
            &journal,
            prepare,
            |_identity, _prepared, _jit| async { Err(HostError::Docker) },
        )
        .await,
        Err(EnsureError::Uncertain)
    );
    assert!(first.calls.is_empty());
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].subject, "m100r42");
    assert_eq!(rows[0].assignment_key.as_deref(), Some("1:42"));
    assert_eq!(rows[0].state, IntentState::Uncertain);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    reopened.bind_engine("docker-engine-migration-test").await?;
    let mut redelivery = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    assert_eq!(
        drive_offer(
            &mut redelivery,
            &ctx(),
            &assignment_poll(101, 42),
            &reopened,
            prepare,
            |_identity, _prepared, _jit| async { Err(HostError::Docker) },
        )
        .await,
        Err(EnsureError::Uncertain)
    );
    assert!(redelivery.calls.is_empty());
    assert_eq!(reopened.rows().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn cleaned_failed_legacy_assignment_retries_once_and_restores_lineage()
-> Result<(), HostError> {
    let scratch = Scratch::new("retry")?;
    let path = scratch.file();
    create_old_schema_row(&path, "m100r42", "failed", true).await?;
    let engine = format!("migration-engine-{}", uuid::Uuid::new_v4().simple());

    let journal = Journal::open(&path).await?;
    let guard = test_engine_lineage_guard(&engine, &scratch.path)?;
    journal.establish_engine_lineage(&engine, guard).await?;
    let LaunchReservation::New(retry) = journal.reserve_assignment(1, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    assert_eq!(
        journal.reserve_assignment(1, 42, 101, 1).await?,
        LaunchReservation::Existing(retry)
    );
    assert_eq!(journal.rows().await?.len(), 2);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let restarted_guard = test_engine_lineage_guard(&engine, &scratch.path)?;
    reopened
        .establish_engine_lineage(&engine, restarted_guard)
        .await?;
    assert_eq!(
        reopened.reserve_assignment(1, 42, 102, 1).await?,
        LaunchReservation::Existing(retry)
    );
    assert_eq!(reopened.rows().await?.len(), 2);
    Ok(())
}
