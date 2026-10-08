//! Monotonic capacity-effect evidence tests.

use std::num::NonZeroU32;
use std::path::Path;

use super::admission::identity;
use crate::journal::{CapacityClaim, Journal, LaunchEffectState};
use crate::{HostError, IntentState, Outcome};

use crate::journal::tests::Scratch;

#[tokio::test]
async fn generic_finish_cannot_rewrite_uncertain_or_done_capacity_reservations()
-> Result<(), String> {
    let scratch = Scratch::new("capacity-monotonic-effect").map_err(|error| error.to_string())?;
    let journal_path = scratch.file();
    let journal = Journal::open(&journal_path)
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let last = identity("https://api.github.com", "one", "session", 3, 3)
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero capacity")?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&first, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first reservation was not new".to_owned());
    };

    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let CapacityClaim::New(done_id) = journal
        .reserve_launch_if_accepting(&next, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("second reservation was not new".to_owned());
    };
    journal
        .finish(done_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let journal = Journal::open(&journal_path)
        .await
        .map_err(|error| error.to_string())?;

    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.finish(id, Outcome::DefiniteFailure).await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_launch_no_effect(id).await,
        Err(HostError::Journal)
    );
    journal
        .finish(done_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.finish(done_id, Outcome::DefiniteFailure).await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_launch_no_effect(done_id).await,
        Err(HostError::Journal)
    );
    assert_eq!(journal.read(id).await, Ok(IntentState::Uncertain));
    assert_eq!(journal.read(done_id).await, Ok(IntentState::Done));
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&last, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 2,
            maximum,
        }
    );
    Ok(())
}

#[tokio::test]
async fn effect_intent_survives_failure_and_cannot_be_reclassified_as_no_effect()
-> Result<(), String> {
    let scratch = Scratch::new("capacity-effect-intent").map_err(|error| error.to_string())?;
    let journal_path = scratch.file();
    let journal = Journal::open(&journal_path)
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero capacity")?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&first, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first reservation was not new".to_owned());
    };

    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let journal = Journal::open(&journal_path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.record_launch_no_effect(id).await,
        Err(HostError::Journal)
    );
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

#[tokio::test]
async fn durable_drain_fences_effect_intents_before_each_dispatch() -> Result<(), String> {
    assert_drain_blocks_effects(false).await?;
    assert_drain_blocks_effects(true).await?;
    Ok(())
}

async fn assert_drain_blocks_effects(after_first_effect: bool) -> Result<(), String> {
    let label = if after_first_effect {
        "capacity-drain-between-effects"
    } else {
        "capacity-drain-before-effect"
    };
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch = identity("https://api.github.com", "one", "session", 11, 101)
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 12, 102)
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero capacity")?;
    let CapacityClaim::New(launch_id) = journal
        .reserve_launch_if_accepting(&launch, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("reservation before drain was not new".to_owned());
    };

    if after_first_effect {
        journal
            .record_launch_effect_intent(launch_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.record_launch_effect_intent(launch_id).await,
        Err(HostError::Journal)
    );
    let expected = if after_first_effect {
        LaunchEffectState::MayHaveEffect
    } else {
        LaunchEffectState::NotStarted
    };
    assert_eq!(journal.launch_effect_state(launch_id).await, Ok(expected));
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened.record_launch_effect_intent(launch_id).await,
        Err(HostError::Journal)
    );
    assert_eq!(reopened.launch_effect_state(launch_id).await, Ok(expected));
    assert_eq!(
        reopened.reserve_launch_if_accepting(&next, maximum).await,
        Ok(CapacityClaim::Draining)
    );
    Ok(())
}

#[tokio::test]
async fn v4_failed_scoped_row_migrates_with_unknown_effect_and_stays_occupied() -> Result<(), String>
{
    let scratch = Scratch::new("capacity-v4-unknown-effect").map_err(|error| error.to_string())?;
    create_v4_failed_scoped_row(&scratch.file()).await?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero capacity")?;

    assert_eq!(
        journal.record_launch_no_effect(1).await,
        Err(HostError::Journal)
    );
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

async fn create_v4_failed_scoped_row(file_path: &Path) -> Result<(), String> {
    let path_text = file_path.to_str().ok_or("journal path")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, message_id INTEGER, runner_request_id INTEGER, requested_workflow_run_id INTEGER, requested_job_id TEXT, runner_name TEXT, observed_job_id TEXT, observed_workflow_run_id INTEGER, remote_terminal INTEGER NOT NULL DEFAULT 0, replay_key_version INTEGER NOT NULL DEFAULT 0 CHECK (replay_key_version IN (0, 1)))",
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
            "INSERT INTO intents (kind, subject, state, replay_key_version) VALUES ('launch', 'scope-v1:old-failed', 'failed', 1)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(database);
    Ok(())
}
