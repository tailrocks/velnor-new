//! V12 logical Docker Engine binding and fail-closed migration tests.

use std::num::NonZeroU32;

use crate::journal::{
    AssignedPopulationObservation, BoundCapacityClaim, CapacityClaim, JournalDockerDaemonBinding,
    ReplayRoute, ScopedAssignedLaunchIdentity, ScopedLaunchIdentity,
};
use crate::{DrainSnapshot, Journal};

use super::Scratch;

fn route() -> ReplayRoute<'static> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner: "acme",
        repository: "widget",
        runner_group_id: 17,
        runner_group_name: "trusted",
        scale_set_id: 31,
        scale_set_name: "linux",
    }
}

fn offer(request_id: i64) -> Result<ScopedLaunchIdentity, crate::HostError> {
    ScopedLaunchIdentity::new(route(), "session-v12", 91, request_id)
}

fn binding(engine_id: &str) -> Result<JournalDockerDaemonBinding, crate::HostError> {
    JournalDockerDaemonBinding::new("/run/docker.sock", engine_id)
}

#[tokio::test]
async fn bound_available_reservation_is_atomic_and_never_rebound() -> Result<(), String> {
    let scratch = Scratch::new("daemon-binding-available").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let identity = offer(1).map_err(|error| error.to_string())?;
    let expected = binding("engine-a").map_err(|error| error.to_string())?;
    let other_engine = binding("engine-b").map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;

    let BoundCapacityClaim::New(id) = journal
        .reserve_linux_launch_if_accepting(&identity, &expected, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first bound reservation was not new".to_owned());
    };
    assert_eq!(
        journal
            .launch_daemon_binding(id)
            .await
            .map_err(|error| error.to_string())?,
        Some(expected.clone())
    );
    assert_eq!(
        journal
            .reserve_linux_launch_if_accepting(&identity, &expected, maximum)
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::Existing(id)
    );
    assert_eq!(
        journal
            .reserve_linux_launch_if_accepting(&identity, &other_engine, maximum)
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::ExistingBindingChanged(id)
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_linux_launch_if_accepting(&identity, &expected, maximum)
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::Existing(id),
        "exact replay remains available after the drain fence"
    );
    assert_eq!(
        journal
            .reserve_linux_launch_if_accepting(
                &offer(2).map_err(|error| error.to_string())?,
                &expected,
                maximum,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::Draining,
        "a new bound identity must observe the shared drain gate"
    );
    Ok(())
}

#[tokio::test]
async fn assigned_bound_reservation_replays_only_on_the_same_engine() -> Result<(), String> {
    let scratch = Scratch::new("daemon-binding-assigned").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let observation = AssignedPopulationObservation::new(1_800_000_000_000, 3, 1)
        .map_err(|error| error.to_string())?;
    let identity = ScopedAssignedLaunchIdentity::new(
        route(),
        7001,
        "session-v12",
        9,
        Some(91),
        observation,
        0,
    )
    .map_err(|error| error.to_string())?;
    let expected = binding("engine-a").map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let BoundCapacityClaim::New(id) = journal
        .reserve_linux_assigned_launch_if_accepting(&identity, &expected, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first assigned reservation was not new".to_owned());
    };
    assert_eq!(
        journal
            .reserve_linux_assigned_launch_if_accepting(&identity, &expected, maximum)
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::Existing(id)
    );
    assert_eq!(
        journal
            .reserve_linux_assigned_launch_if_accepting(
                &identity,
                &binding("engine-b").map_err(|error| error.to_string())?,
                maximum,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::ExistingBindingChanged(id)
    );
    assert_eq!(
        journal
            .launch_daemon_binding(id)
            .await
            .map_err(|error| error.to_string())?,
        Some(expected)
    );
    Ok(())
}

#[tokio::test]
async fn v11_upgrade_keeps_rows_unbound_and_global_capacity_occupied() -> Result<(), String> {
    let scratch =
        Scratch::new("daemon-binding-v11-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let identity = offer(1).map_err(|error| error.to_string())?;
    let id = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        let CapacityClaim::New(id) = journal
            .reserve_launch_if_accepting(&identity, NonZeroU32::new(2).ok_or("nonzero maximum")?)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("legacy fixture row was not created".to_owned());
        };
        id
    };
    set_v11_schema(&path).await?;

    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        migrated
            .launch_daemon_binding(id)
            .await
            .map_err(|error| error.to_string())?,
        None
    );
    assert_eq!(
        migrated
            .reserve_linux_launch_if_accepting(
                &identity,
                &binding("engine-a").map_err(|error| error.to_string())?,
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::ExistingUnbound(id)
    );
    assert_eq!(
        migrated
            .reserve_linux_launch_if_accepting(
                &offer(2).map_err(|error| error.to_string())?,
                &binding("engine-a").map_err(|error| error.to_string())?,
                NonZeroU32::new(1).ok_or("nonzero maximum")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::CapacityFull {
            occupied: 1,
            maximum: NonZeroU32::new(1).ok_or("nonzero maximum")?,
        }
    );
    assert_eq!(
        migrated
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 1,
            unresolved_intents: 0,
        }
    );
    Ok(())
}

#[tokio::test]
async fn binding_insert_failure_rolls_back_the_new_intent() -> Result<(), String> {
    let scratch = Scratch::new("daemon-binding-atomic").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let database = turso::Builder::new_local(path.to_str().ok_or("journal path was not UTF-8")?)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_bindings", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    assert!(
        journal
            .reserve_linux_launch_if_accepting(
                &offer(3).map_err(|error| error.to_string())?,
                &binding("engine-a").map_err(|error| error.to_string())?,
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .is_err()
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        0,
        "the failed binding insert must roll back the preceding intent"
    );
    Ok(())
}

async fn set_v11_schema(path: &std::path::Path) -> Result<(), String> {
    let database = turso::Builder::new_local(path.to_str().ok_or("journal path was not UTF-8")?)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_bindings", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}
