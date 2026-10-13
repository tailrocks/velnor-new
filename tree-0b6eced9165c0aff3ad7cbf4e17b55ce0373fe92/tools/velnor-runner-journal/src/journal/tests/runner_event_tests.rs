//! Event-to-generation correlation and legacy identity migration tests.

use std::path::Path;

use velnor_runner_github::{InnerJob, InnerKind};

use crate::journal::LaunchClaim;
use crate::{HostError, IntentState, Journal, Outcome};

use super::Scratch;

#[tokio::test]
async fn only_correlated_remote_completion_resolves_a_launch() -> Result<(), String> {
    let scratch = Scratch::new("remote-terminal").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (id, fresh) = journal
        .begin_launch("m7r19")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_launch_identity(id, Some(7), Some(19), Some(23), Some("opaque-job"), "v19")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;

    // AcquireJobs records the requested job, but GitHub assigns work to an
    // eligible idle runner and does not bind that request to this JIT name.
    let started = runner_event(InnerKind::Started, "v19", 88, "actual-job", 45);
    assert!(
        journal
            .observe_runner_event(&started)
            .await
            .map_err(|error| error.to_string())?
    );
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .remove(0);
    assert_eq!(row.message_id, Some(7));
    assert_eq!(row.runner_request_id, Some(19));
    assert_eq!(row.requested_workflow_run_id, Some(23));
    assert_eq!(row.requested_job_id.as_deref(), Some("opaque-job"));
    assert_eq!(row.github_runner_id.as_deref(), Some("88"));
    assert_eq!(row.observed_workflow_run_id, Some(45));
    assert_eq!(row.observed_job_id.as_deref(), Some("actual-job"));
    assert!(!row.remote_terminal);

    let mismatch = runner_event(InnerKind::Completed, "v19", 88, "other-job", 45);
    assert!(matches!(
        journal.observe_runner_event(&mismatch).await,
        Err(HostError::Journal)
    ));
    assert!(!journal.rows().await.map_err(|error| error.to_string())?[0].remote_terminal);

    let completed = runner_event(InnerKind::Completed, "v19", 88, "actual-job", 45);
    assert!(
        journal
            .observe_runner_event(&completed)
            .await
            .map_err(|error| error.to_string())?
    );
    assert!(journal.rows().await.map_err(|error| error.to_string())?[0].remote_terminal);
    assert!(journal.record_cleanup(id).await.is_err());
    assert_eq!(
        journal
            .begin_launch_if_accepting("m7r19")
            .await
            .map_err(|error| error.to_string())?,
        LaunchClaim::Existing(id)
    );
    Ok(())
}

#[tokio::test]
async fn late_event_cannot_terminalize_a_reused_runner_generation() -> Result<(), String> {
    let scratch = Scratch::new("runner-generation-reuse").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (old_id, _) = journal
        .begin_launch("old-generation")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_launch_identity(old_id, None, None, None, None, "v-old")
        .await
        .map_err(|error| error.to_string())?;
    let old_started = runner_event(InnerKind::Started, "v-old", 88, "old-job", 45);
    journal
        .observe_runner_event(&old_started)
        .await
        .map_err(|error| error.to_string())?;
    let old_completed = runner_event(InnerKind::Completed, "v-old", 88, "old-job", 45);
    journal
        .observe_runner_event(&old_completed)
        .await
        .map_err(|error| error.to_string())?;
    assert!(journal.record_cleanup(old_id).await.is_err());

    let (new_id, _) = journal
        .begin_launch("new-generation")
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .bind_launch_identity(new_id, None, None, None, None, "v-old")
            .await
            .is_err()
    );
    inject_legacy_runner_name_reuse(&scratch.file(), new_id).await?;

    assert!(matches!(
        journal.observe_runner_event(&old_completed).await,
        Err(HostError::Journal)
    ));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    let old = rows
        .iter()
        .find(|row| row.id == old_id)
        .ok_or_else(|| "old launch row is missing".to_owned())?;
    let new = rows
        .iter()
        .find(|row| row.id == new_id)
        .ok_or_else(|| "new launch row is missing".to_owned())?;
    assert!(old.remote_terminal);
    assert!(!new.remote_terminal);
    assert_eq!(new.github_runner_id, None);
    Ok(())
}

async fn inject_legacy_runner_name_reuse(path: &std::path::Path, id: i64) -> Result<(), String> {
    let db = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "temporary path is not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE intents SET runner_name = 'v-old' WHERE id = ?1",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn migrated_unknown_runner_name_does_not_match_late_legacy_event() -> Result<(), String> {
    let scratch = Scratch::new("journal-v2-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    seed_v2_legacy_launch(&path).await?;

    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let legacy = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == 1)
        .ok_or_else(|| "legacy launch row is missing".to_owned())?;
    assert_eq!(legacy.state, IntentState::Uncertain);
    assert_eq!(legacy.runner_name, None);
    assert!(!legacy.remote_terminal);
    assert!(!legacy.cleanup_proven);

    let (id, _) = journal
        .begin_launch("legacy-row")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(id, 2);
    journal
        .bind_launch_identity(id, Some(1), Some(19), Some(46), Some("new-job"), "g2_2")
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        !journal
            .observe_runner_event(&runner_event(InnerKind::Completed, "v2", 88, "old-job", 45))
            .await
            .map_err(|error| error.to_string())?
    );
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| "migrated row is missing".to_owned())?;
    assert_eq!(row.message_id, Some(1));
    assert_eq!(row.runner_request_id, Some(19));
    assert_eq!(row.requested_workflow_run_id, Some(46));
    assert_eq!(row.requested_job_id.as_deref(), Some("new-job"));
    assert_eq!(row.observed_workflow_run_id, None);
    assert_eq!(row.observed_job_id, None);
    assert_eq!(row.runner_name.as_deref(), Some("g2_2"));
    assert_eq!(row.github_runner_id, None);
    assert!(!row.remote_terminal);
    Ok(())
}

async fn seed_v2_legacy_launch(path: &Path) -> Result<(), String> {
    let db = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "temporary path is not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE controller_state (id INTEGER PRIMARY KEY CHECK (id = 1), draining INTEGER NOT NULL DEFAULT 0 CHECK (draining IN (0, 1)), drain_requested_at_ms INTEGER)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO controller_state (id, draining, drain_requested_at_ms) VALUES (1, 0, NULL)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (id, kind, subject, state, cleanup_proven) VALUES (1, 'launch', 'legacy-live', 'uncertain', 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);
    Ok(())
}

fn runner_event(
    kind: InnerKind,
    name: &str,
    runner_id: i64,
    job_id: &str,
    run_id: i64,
) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some(job_id.to_owned()),
        workflow_run_id: Some(run_id),
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: Some(runner_id),
        runner_name: Some(name.to_owned()),
        result: None,
        fields: Vec::new(),
    }
}
