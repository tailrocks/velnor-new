//! Atomic cross-scope Available reservations with exact daemon bindings.

use std::num::NonZeroU32;

use crate::Journal;
use crate::journal::{
    BoundBatchCapacityClaim, BoundCapacityClaim, JournalDockerDaemonBinding, ReplayRoute,
    ScopedLaunchIdentity,
};

use super::Scratch;

fn route(repository: &'static str, scale_set_id: i64) -> ReplayRoute<'static> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner: "acme",
        repository,
        runner_group_id: 17,
        runner_group_name: "trusted",
        scale_set_id,
        scale_set_name: "linux",
    }
}

fn offer(
    repository: &'static str,
    scale_set_id: i64,
    session_id: &'static str,
) -> Result<ScopedLaunchIdentity, crate::HostError> {
    ScopedLaunchIdentity::new(route(repository, scale_set_id), session_id, 7, 11)
}

fn binding() -> Result<JournalDockerDaemonBinding, crate::HostError> {
    JournalDockerDaemonBinding::new("/run/docker.sock", "engine-a")
}

#[tokio::test]
async fn same_message_request_pair_in_distinct_scopes_reserves_and_replays_separately()
-> Result<(), String> {
    let scratch = Scratch::new("bound-batch-scopes").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let engine = binding().map_err(|error| error.to_string())?;
    let first = offer("widget-a", 32, "session-a").map_err(|error| error.to_string())?;
    let second = offer("widget-b", 33, "session-b").map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let BoundBatchCapacityClaim::Offers(reserved) = journal
        .reserve_linux_launch_batch_all_or_none_if_accepting(
            &[first.clone(), second.clone()],
            &engine,
            maximum,
        )
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("batch did not fit".to_owned());
    };
    let [
        BoundCapacityClaim::New(first_id),
        BoundCapacityClaim::New(second_id),
    ] = reserved.as_slice()
    else {
        return Err(format!("expected two new scoped claims, got {reserved:?}"));
    };
    assert_ne!(first_id, second_id);
    for id in [first_id, second_id] {
        assert_eq!(
            journal
                .launch_daemon_binding(*id)
                .await
                .map_err(|error| error.to_string())?,
            Some(engine.clone())
        );
    }

    let BoundBatchCapacityClaim::Offers(replayed) = journal
        .reserve_linux_launch_batch_all_or_none_if_accepting(&[second, first], &engine, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("exact scoped batch replay was not returned".to_owned());
    };
    assert_eq!(
        replayed.as_slice(),
        &[
            BoundCapacityClaim::Existing(*second_id),
            BoundCapacityClaim::Existing(*first_id)
        ]
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn insufficient_global_capacity_inserts_no_scoped_batch_rows() -> Result<(), String> {
    let scratch = Scratch::new("bound-batch-capacity").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let blocker = offer("blocker", 31, "session-blocker").map_err(|error| error.to_string())?;
    let first = offer("widget-a", 32, "session-a").map_err(|error| error.to_string())?;
    let second = offer("widget-b", 33, "session-b").map_err(|error| error.to_string())?;
    let engine = binding().map_err(|error| error.to_string())?;
    let BoundCapacityClaim::New(blocker_id) = journal
        .reserve_linux_launch_if_accepting(
            &blocker,
            &engine,
            NonZeroU32::new(2).ok_or("nonzero maximum")?,
        )
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("global-capacity blocker was not newly reserved".to_owned());
    };

    assert_eq!(
        journal
            .reserve_linux_launch_batch_all_or_none_if_accepting(
                &[first, second],
                &engine,
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundBatchCapacityClaim::CapacityFull {
            occupied: 1,
            required: 2,
            available: 1,
            maximum: NonZeroU32::new(2).ok_or("nonzero maximum")?,
        }
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, blocker_id);
    Ok(())
}

#[tokio::test]
async fn second_bound_insert_failure_rolls_back_the_whole_batch() -> Result<(), String> {
    let scratch = Scratch::new("bound-batch-rollback").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let database = turso::Builder::new_local(
        scratch
            .file()
            .to_str()
            .ok_or("journal path was not UTF-8")?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute(
        "CREATE TRIGGER reject_second_bound_batch_binding BEFORE INSERT ON linux_launch_daemon_bindings WHEN (SELECT COUNT(*) FROM intents WHERE kind = 'launch') >= 2 BEGIN SELECT RAISE(ABORT, 'injected bound batch fault'); END",
        (),
    )
    .await
    .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    let identities = [
        offer("widget-a", 32, "session-a").map_err(|error| error.to_string())?,
        offer("widget-b", 33, "session-b").map_err(|error| error.to_string())?,
    ];
    assert!(
        journal
            .reserve_linux_launch_batch_all_or_none_if_accepting(
                &identities,
                &binding().map_err(|error| error.to_string())?,
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .is_err(),
        "second binding insertion should trigger a transactional failure"
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        0
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        0
    );
    Ok(())
}

mod mixed;
