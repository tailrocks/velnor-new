//! Global capacity reservation and replay-isolation tests.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use velnor_runner_github::{InnerJob, InnerKind};

use crate::journal::{CapacityClaim, Journal, ReplayRoute, ScopedLaunchIdentity};
use crate::{HostError, IntentState, Outcome};

use crate::journal::tests::Scratch;

pub(super) fn identity(
    destination: &str,
    repository: &str,
    session_id: &str,
    message_id: i64,
    request_id: i64,
) -> Result<ScopedLaunchIdentity, HostError> {
    ScopedLaunchIdentity::new(
        ReplayRoute {
            destination,
            registration_scope: "repository",
            owner: "example",
            repository,
            runner_group_id: 17,
            runner_group_name: "restricted",
            scale_set_id: 31,
            scale_set_name: "linux-private",
        },
        session_id,
        message_id,
        request_id,
    )
}

#[tokio::test]
async fn global_limit_counts_rows_across_routes_and_fails_closed_on_cleanup_flag()
-> Result<(), String> {
    let scratch = Scratch::new("capacity-global").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let second = identity("https://github.example.test/api", "two", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(1).ok_or("nonzero limit")?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&first, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first reservation was not new".to_owned());
    };

    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_launch_identity(
            id,
            Some(1),
            Some(1),
            Some(88),
            Some("requested-job"),
            "runner-1",
        )
        .await
        .map_err(|error| error.to_string())?;
    for kind in [InnerKind::Started, InnerKind::Completed] {
        journal
            .observe_runner_event(&runner_event(kind))
            .await
            .map_err(|error| error.to_string())?;
    }
    journal
        .record_cleanup(id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(journal.read(id).await, Ok(IntentState::Done));
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&second, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 1,
            maximum: limit,
        }
    );
    Ok(())
}

#[tokio::test]
async fn legacy_rows_occupy_capacity_even_if_failed_and_cleanup_flagged() -> Result<(), String> {
    let scratch = Scratch::new("capacity-legacy").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let legacy = journal
        .begin_launch("m77")
        .await
        .map_err(|error| error.to_string())?
        .0;
    journal
        .finish(legacy, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup(legacy)
        .await
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 78, 2)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&next, NonZeroU32::new(1).ok_or("nonzero")?)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 1,
            maximum: NonZeroU32::new(1).ok_or("nonzero")?,
        }
    );
    Ok(())
}

#[tokio::test]
async fn v3_scope_looking_legacy_subject_migrates_as_unscoped() -> Result<(), String> {
    let scratch = Scratch::new("capacity-v3-legacy").map_err(|error| error.to_string())?;
    create_v3_legacy_journal(&scratch.file()).await?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
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

#[tokio::test]
async fn scoped_definite_no_effect_frees_capacity_but_exact_replay_stays_existing()
-> Result<(), String> {
    let scratch = Scratch::new("capacity-no-effect").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(1).ok_or("nonzero limit")?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&first, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first reservation was not new".to_owned());
    };
    journal
        .record_launch_no_effect(id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&first, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Existing(id)
    );
    assert!(matches!(
        journal
            .reserve_launch_if_accepting(&next, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    Ok(())
}

#[tokio::test]
async fn definite_failure_with_registered_runner_still_occupies_capacity() -> Result<(), String> {
    let scratch = Scratch::new("capacity-runner-observed").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
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
        .bind(id, None, Some("github-runner-1"))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
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
async fn drain_fence_rejects_new_identity_but_preserves_existing_replay() -> Result<(), String> {
    let scratch = Scratch::new("capacity-drain").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let next = identity("https://api.github.com", "one", "session", 2, 2)
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(1).ok_or("nonzero limit")?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&first, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first reservation was not new".to_owned());
    };
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&first, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Existing(id)
    );
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&next, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Draining
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_global_reservations_cannot_oversubscribe() -> Result<(), String> {
    let scratch = Scratch::new("capacity-concurrent").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first_journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second_journal = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "session-a", 1, 1)
        .map_err(|error| error.to_string())?;
    let second = identity("https://api.github.com", "two", "session-b", 2, 2)
        .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(1).ok_or("nonzero limit")?;
    let first_call = reserve_after(first_journal, first, barrier.clone(), maximum);
    let second_call = reserve_after(second_journal, second, barrier.clone(), maximum);
    let first_wait = barrier.wait();
    let (first_result, second_result, _) = tokio::join!(first_call, second_call, first_wait);
    let new_count = usize::from(matches!(first_result, Ok(CapacityClaim::New(_))))
        + usize::from(matches!(second_result, Ok(CapacityClaim::New(_))));
    assert!(
        new_count <= 1,
        "two reservations won: {first_result:?} {second_result:?}"
    );

    let rows = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?
        .rows()
        .await
        .map_err(|error| error.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| row.kind == "launch" && row.state != IntentState::Failed)
        .count();
    assert!(occupied <= 1, "global limit was exceeded: {occupied}");
    Ok(())
}

async fn reserve_after(
    journal: Journal,
    identity: ScopedLaunchIdentity,
    barrier: Arc<tokio::sync::Barrier>,
    maximum: NonZeroU32,
) -> Result<CapacityClaim, HostError> {
    barrier.wait().await;
    journal
        .reserve_launch_if_accepting(&identity, maximum)
        .await
}

fn runner_event(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some("actual-job".to_owned()),
        workflow_run_id: Some(88),
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: Some(701),
        runner_name: Some("runner-1".to_owned()),
        result: None,
        fields: Vec::new(),
    }
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
