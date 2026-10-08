use std::num::NonZeroU32;
use std::path::Path;

use crate::journal::tests::Scratch;
use crate::journal::{CapacityClaim, Journal};

use super::identity;

#[tokio::test]
async fn v3_scope_looking_legacy_subject_migrates_as_unscoped() -> Result<(), String> {
    let scratch = Scratch::new("capacity-v3-legacy").map_err(|error| error.to_string())?;
    create_v3_legacy_journal(&scratch.file()).await?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let legacy = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.subject == "scope-v1:legacy-looking")
        .ok_or("legacy row missing after migration")?;
    assert!(!legacy.cleanup_proven);

    let next = identity("https://api.github.com", "one", "session", 78, 2)
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero limit")?;
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&next, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 1,
            maximum,
        }
    );
    Ok(())
}

async fn create_v3_legacy_journal(path: &Path) -> Result<(), String> {
    let path_text = path.to_str().ok_or("journal path")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, message_id INTEGER, runner_request_id INTEGER, requested_workflow_run_id INTEGER, requested_job_id TEXT, runner_name TEXT, observed_job_id TEXT, observed_workflow_run_id INTEGER, remote_terminal INTEGER NOT NULL DEFAULT 0)",
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
            "INSERT INTO controller_state (id, draining) VALUES (1, 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 'scope-v1:legacy-looking', 'failed', 1)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 3", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(database);
    Ok(())
}
