//! A journal upgrade must not retry ambiguous failures from older binaries.

use std::path::Path;

use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{Mode, Scratch, Script, assigned_wait};
use crate::{IntentState, Journal, Outcome};

async fn seed_legacy_journal(path: &Path) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or_else(|| "legacy journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 'm96', 'failed', 1), ('acquire', 'old-acquire', 'failed', 1)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);
    Ok(())
}

#[tokio::test]
async fn legacy_failed_launch_migrates_before_restart_can_mint() -> Result<(), String> {
    let scratch = Scratch::new("legacy-failed-launch").map_err(|error| error.to_string())?;
    let path = scratch.file();
    seed_legacy_journal(&path).await?;
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    let legacy_launch = rows
        .iter()
        .find(|row| row.subject == "m96")
        .ok_or_else(|| "legacy launch row disappeared".to_owned())?;
    assert_eq!(legacy_launch.state, IntentState::Uncertain);
    assert!(!legacy_launch.cleanup_proven);
    let unrelated = rows
        .iter()
        .find(|row| row.subject == "old-acquire")
        .ok_or_else(|| "unrelated row disappeared".to_owned())?;
    assert_eq!(unrelated.state, IntentState::Failed);
    assert!(unrelated.cleanup_proven);

    let session = super::zero_assignment_session()?;
    let polled = assigned_wait(96, 1);
    let docker = DockerStub::open(Vec::new())?;
    let decision = crate::launch::admission(&docker.docker, &journal, 1, 1, 0, &polled)
        .await
        .map_err(|error| error.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers = Vec::new();
    let result = super::start_turn(
        &mut script,
        &mut workers,
        super::ready(&session, &polled),
        &journal,
        &docker.docker,
        1,
        false,
    )
    .await;
    docker.finish().await?;

    assert_eq!(decision, crate::launch::Admit::Hold);
    assert_eq!(result, Ok(false));
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));

    let fresh = journal
        .begin_launch("m97")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh.1);
    journal
        .finish(fresh.0, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let persisted = reopened.rows().await.map_err(|error| error.to_string())?;
    let definite = persisted
        .iter()
        .find(|row| row.subject == "m97")
        .ok_or_else(|| "new definite failure disappeared".to_owned())?;
    assert_eq!(definite.state, IntentState::Failed);
    let retry = reopened
        .begin_launch("m97")
        .await
        .map_err(|error| error.to_string())?;
    assert!(retry.1);
    assert_ne!(retry.0, fresh.0);
    Ok(())
}
