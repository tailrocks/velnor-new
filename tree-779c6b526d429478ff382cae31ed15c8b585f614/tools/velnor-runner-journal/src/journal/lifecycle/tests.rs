//! Crash and replay tests for private-network and runner-start intent.

use std::num::NonZeroU32;

use crate::journal::{
    CapacityClaim, Journal, LaunchEffectState, ReplayRoute, RunnerStartIntent, ScopedLaunchIdentity,
};
use crate::{HostError, journal::tests::Scratch};

fn identity(message_id: i64, request_id: i64) -> Result<ScopedLaunchIdentity, HostError> {
    ScopedLaunchIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "organization",
            owner: "velnor",
            repository: "",
            runner_group_id: 7,
            runner_group_name: "linux",
            scale_set_id: 11,
            scale_set_name: "workers",
        },
        "session-full-id",
        message_id,
        request_id,
    )
}

#[tokio::test]
async fn private_network_intent_survives_reopen_and_conflicts() -> Result<(), String> {
    let scratch = Scratch::new("launch-lifecycle-network").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let id = reserve(&journal, 1, 101).await?;
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_outer_network_intent(id, "velnor-net-worker-abc")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("launch row after reopen")?;
    assert_eq!(
        row.outer_network_name.as_deref(),
        Some("velnor-net-worker-abc")
    );
    assert_eq!(row.outer_network_id, None);
    assert_eq!(row.launch_effect, LaunchEffectState::MayHaveEffect);
    assert_eq!(row.runner_start_intent, RunnerStartIntent::NotRequested);
    let next = identity(2, 102).map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("capacity")?;
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

    journal
        .bind_outer_network_id(id, "a1b2c3d4")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_outer_network_id(id, "a1b2c3d4")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.bind_outer_network_id(id, "deadbeef").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal
            .record_outer_network_intent(id, "velnor-net-other")
            .await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn runner_start_intent_survives_reopen_and_requires_exact_id() -> Result<(), String> {
    let scratch =
        Scratch::new("launch-lifecycle-runner-start").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let id = reserve(&journal, 4, 104).await?;
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(id, Some("0a0b0c0d"), None)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_start_intent(id, "0a0b0c0d")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.bind(id, Some("11121314"), None).await,
        Err(HostError::Journal)
    );
    journal
        .bind(id, Some("0a0b0c0d"), None)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_start_intent(id, "0a0b0c0d")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.record_runner_start_intent(id, "11121314").await,
        Err(HostError::Journal)
    );
    drop(journal);

    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .runner_start_intent(id)
            .await
            .map_err(|error| error.to_string())?,
        RunnerStartIntent::MayHaveStarted
    );
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("launch row after second reopen")?;
    assert_eq!(row.outer_network_id, None);
    assert_eq!(row.runner_start_intent, RunnerStartIntent::MayHaveStarted);
    assert_eq!(
        journal.record_launch_no_effect(id).await,
        Err(HostError::Journal)
    );
    Ok(())
}

async fn reserve(journal: &Journal, message_id: i64, request_id: i64) -> Result<i64, String> {
    let identity = identity(message_id, request_id).map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("capacity")?;
    match journal
        .reserve_launch_if_accepting(&identity, maximum)
        .await
        .map_err(|error| error.to_string())?
    {
        CapacityClaim::New(id) => Ok(id),
        _ => Err("launch was not newly reserved".to_owned()),
    }
}

#[tokio::test]
async fn runner_start_requires_an_exact_bound_id() -> Result<(), String> {
    let scratch =
        Scratch::new("launch-lifecycle-start-fail-closed").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let legacy_id = journal
        .begin("launch", "legacy-row")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .runner_start_intent(legacy_id)
            .await
            .map_err(|error| error.to_string())?,
        RunnerStartIntent::NotRequested
    );
    journal
        .record_launch_effect_intent(legacy_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_runner_start_intent(legacy_id, "abcdef01")
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal
            .record_outer_network_intent(legacy_id, "bad/name")
            .await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn v5_uncertain_launch_migrates_without_network_or_start_proof() -> Result<(), String> {
    let scratch =
        Scratch::new("launch-lifecycle-v5-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let path_text = path.to_str().ok_or("journal path")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, message_id INTEGER, runner_request_id INTEGER, requested_workflow_run_id INTEGER, requested_job_id TEXT, runner_name TEXT, observed_job_id TEXT, observed_workflow_run_id INTEGER, remote_terminal INTEGER NOT NULL DEFAULT 0, replay_key_version INTEGER NOT NULL DEFAULT 0 CHECK (replay_key_version IN (0, 1)), effect_state TEXT NOT NULL DEFAULT 'unknown' CHECK (effect_state IN ('unknown', 'not_started', 'may_have_effect', 'definite_no_effect')))",
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
            "INSERT INTO intents (id, kind, subject, state, replay_key_version, effect_state) VALUES (1, 'launch', 'scope-v1:old-uncertain', 'uncertain', 1, 'may_have_effect')",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(database);

    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    let row = rows.first().ok_or("migrated launch")?;
    assert_eq!(row.id, 1);
    assert_eq!(row.launch_effect, LaunchEffectState::MayHaveEffect);
    assert_eq!(row.outer_network_name, None);
    assert_eq!(row.outer_network_id, None);
    assert_eq!(row.runner_start_intent, RunnerStartIntent::UnknownLegacy);
    let next = identity(3, 103).map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("capacity")?;
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
