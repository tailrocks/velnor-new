//! Completion identity and migration regression tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::{CompletedLaunch, CompletionIdentity};
use crate::{HostError, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-completion-{label}-{}-{id}",
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

async fn create_old_schema(path: &Path) -> Result<(), HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
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
            "INSERT INTO intents (kind, subject, state) VALUES ('launch', 'm90r600', 'done')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

fn assert_completion_identity(row: &CompletedLaunch) {
    assert_eq!(
        row.identity,
        CompletionIdentity {
            scale_set_id: 77,
            runner_request_id: 600,
            runner_id: 81,
            runner_name: "v600".to_owned(),
            runner_absent: false,
        }
    );
}

#[tokio::test]
async fn legacy_schema_migrates_idempotently_and_backfills_exact_assignment()
-> Result<(), HostError> {
    let scratch = Scratch::new("migration")?;
    let path = scratch.file();
    create_old_schema(&path).await?;
    let journal = Journal::open(&path).await?;
    let row_id = journal.rows().await?.first().ok_or(HostError::Journal)?.id;
    let (reused_id, fresh) = journal
        .begin_assigned_launch("m91r600", 77, 600, "v600")
        .await?;
    assert_eq!(reused_id, row_id);
    assert!(!fresh);
    assert_eq!(
        journal.record_runner_completed(77, 600, 81, "v600").await?,
        Some(row_id)
    );
    let reopened = Journal::open(&path).await?;
    let due = reopened.due_completed_launches(0, 10).await?;
    assert_eq!(due.len(), 1);
    assert_completion_identity(&due[0]);
    Ok(())
}

#[tokio::test]
async fn assigned_completion_is_idempotent_and_conflicting_identity_fails() -> Result<(), HostError>
{
    let scratch = Scratch::new("identity")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, fresh) = journal
        .begin_assigned_launch("m101r42", 9, 42, "v42")
        .await?;
    assert!(fresh);
    let (reused, fresh) = journal
        .begin_assigned_launch("m102r42", 9, 42, "v42")
        .await?;
    assert_eq!(reused, id);
    assert!(!fresh);
    assert_eq!(
        journal.record_runner_completed(9, 42, 501, "v42").await?,
        Some(id)
    );
    assert_eq!(
        journal.record_runner_completed(9, 42, 501, "v42").await?,
        Some(id)
    );
    assert_eq!(
        journal.record_runner_completed(9, 42, 502, "v42").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_runner_completed(9, 43, 501, "v43").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_runner_completed(10, 42, 503, "v42").await?,
        None
    );
    Ok(())
}

#[tokio::test]
async fn ambiguous_legacy_request_subjects_are_not_guessed() -> Result<(), HostError> {
    let scratch = Scratch::new("ambiguous")?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.begin("launch", "m1r51").await?;
    journal.begin("launch", "m2r51").await?;
    assert_eq!(
        journal.record_runner_completed(9, 51, 801, "v51").await,
        Err(HostError::Journal)
    );
    assert!(journal.due_completed_launches(0, 10).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn session_completion_requires_persisted_set_and_exact_name() -> Result<(), HostError> {
    let scratch = Scratch::new("session-identity")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, _) = journal.begin_launch("sabc123").await?;
    journal
        .bind_launch_identity(id, 77, None, "sabc123")
        .await?;
    assert_eq!(
        journal
            .record_runner_completed(77, 42, 901, "sabc123")
            .await?,
        Some(id)
    );
    let due = journal.due_completed_launches(0, 10).await?;
    assert_eq!(due[0].identity.scale_set_id, 77);
    assert_eq!(due[0].identity.runner_request_id, 42);
    assert_eq!(due[0].identity.runner_name, "sabc123");
    assert_eq!(
        journal
            .record_runner_completed(78, 43, 902, "sabc123")
            .await?,
        None
    );
    Ok(())
}

#[tokio::test]
async fn done_session_identity_rebind_is_idempotent_and_conflicts_fail() -> Result<(), HostError> {
    let scratch = Scratch::new("done-session-rebind")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, fresh) = journal.begin_launch("sbound123").await?;
    assert!(fresh);
    journal
        .bind_launch_identity(id, 77, None, "sbound123")
        .await?;
    journal.finish(id, Outcome::Done).await?;

    let (replayed, fresh) = journal.begin_launch("sbound123").await?;
    assert_eq!(replayed, id);
    assert!(!fresh);
    journal
        .bind_launch_identity(replayed, 77, None, "sbound123")
        .await?;
    assert_eq!(
        journal
            .bind_launch_identity(replayed, 78, None, "sbound123")
            .await,
        Err(HostError::Journal)
    );
    journal
        .bind_launch_identity(replayed, 77, None, "sbound123")
        .await?;
    Ok(())
}

#[tokio::test]
async fn message_named_statistics_runner_can_bind_completion_identity() -> Result<(), HostError> {
    let scratch = Scratch::new("message-runner")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, _) = journal.begin_launch("m25").await?;
    journal.bind_launch_identity(id, 77, None, "m25").await?;

    assert_eq!(
        journal.record_runner_completed(77, 601, 903, "m25").await?,
        Some(id)
    );
    let due = journal.due_completed_launches(0, 10).await?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].identity.runner_request_id, 601);
    assert_eq!(due[0].identity.runner_name, "m25");
    Ok(())
}

#[tokio::test]
async fn quarantined_completion_can_match_a_failed_legacy_assignment() -> Result<(), HostError> {
    let scratch = Scratch::new("legacy-failed-completion")?;
    let path = scratch.file();
    create_old_schema(&path).await?;
    let journal = Journal::open(&path).await?;
    let row = journal.rows().await?.first().ok_or(HostError::Journal)?.id;
    journal.finish(row, Outcome::DefiniteFailure).await?;

    assert_eq!(
        journal.record_runner_completed(77, 600, 81, "v600").await?,
        Some(row)
    );
    let reopened = Journal::open(&path).await?;
    let due = reopened.due_completed_launches(i64::MAX, 10).await?;
    assert_eq!(due.len(), 1);
    assert_completion_identity(&due[0]);
    Ok(())
}

#[tokio::test]
async fn cleaned_failed_history_does_not_hide_the_current_request_generation()
-> Result<(), HostError> {
    let scratch = Scratch::new("generation")?;
    let journal = Journal::open(&scratch.file()).await?;
    let old = journal.begin("launch", "m9r61").await?;
    journal.finish(old, Outcome::DefiniteFailure).await?;
    journal.record_cleanup(old).await?;
    let (current, fresh) = journal
        .begin_assigned_launch("m10r61", 77, 61, "v61")
        .await?;
    assert_ne!(current, old);
    assert!(fresh);
    let (reused, fresh) = journal
        .begin_assigned_launch("m11r61", 77, 61, "v61")
        .await?;
    assert_eq!(reused, current);
    assert!(!fresh);
    Ok(())
}
