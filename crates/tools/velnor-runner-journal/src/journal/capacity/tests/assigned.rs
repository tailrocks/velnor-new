//! Assigned-population slot identity and global-capacity regressions.

use std::num::NonZeroU32;
use std::sync::Arc;

use crate::journal::tests::Scratch;
use crate::journal::{
    AssignedPopulationObservation, CapacityClaim, Journal, ReplayRoute,
    ScopedAssignedLaunchIdentity,
};
use crate::{HostError, Outcome};

fn route(owner: &'static str, repository: &'static str) -> ReplayRoute<'static> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner,
        repository,
        runner_group_id: 17,
        runner_group_name: "restricted",
        scale_set_id: 31,
        scale_set_name: "ubuntu-26.04-scale-set",
    }
}

fn observation(
    observed_at_ms: u128,
    assigned_jobs: u32,
    running_jobs: u32,
) -> Result<AssignedPopulationObservation, HostError> {
    AssignedPopulationObservation::new(observed_at_ms, assigned_jobs, running_jobs)
}

#[derive(Clone, Copy)]
struct IdentityInput<'a> {
    owner: &'static str,
    repository: &'static str,
    target_repository_id: i64,
    session_id: &'a str,
    demand_id: u64,
    message_id: Option<i64>,
    sample: AssignedPopulationObservation,
    ordinal: u64,
}

fn identity(input: IdentityInput<'_>) -> Result<ScopedAssignedLaunchIdentity, HostError> {
    ScopedAssignedLaunchIdentity::new(
        route(input.owner, input.repository),
        input.target_repository_id,
        input.session_id,
        input.demand_id,
        input.message_id,
        input.sample,
        input.ordinal,
    )
}

#[tokio::test]
async fn reopen_and_renamed_route_replay_same_stable_demand_ordinal() -> Result<(), String> {
    let scratch = Scratch::new("assigned-reopen").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let sample = observation(1_800_000_000_000, 5, 2).map_err(|error| error.to_string())?;
    let first = identity(IdentityInput {
        owner: "acme",
        repository: "before-rename",
        target_repository_id: 7001,
        session_id: "session-a",
        demand_id: 9,
        message_id: Some(41),
        sample,
        ordinal: 0,
    })
    .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let CapacityClaim::New(id) = journal
        .reserve_assigned_launch_if_accepting(&first, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first assigned slot was not reserved".to_owned());
    };
    drop(journal);

    let reopened = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let changed_sample = observation(1_800_000_000_001, 6, 2).map_err(|error| error.to_string())?;
    let renamed = identity(IdentityInput {
        owner: "acme-renamed",
        repository: "after-rename",
        target_repository_id: 7001,
        session_id: "session-a",
        demand_id: 9,
        message_id: Some(41),
        sample: changed_sample,
        ordinal: 0,
    })
    .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .reserve_assigned_launch_if_accepting(&renamed, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Existing(id)
    );
    assert_eq!(
        reopened
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    Ok(())
}

#[tokio::test]
async fn distinct_ordinals_use_global_capacity_and_no_effect_can_release_only_its_row()
-> Result<(), String> {
    let scratch = Scratch::new("assigned-capacity").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let sample = observation(1_800_000_000_000, 4, 1).map_err(|error| error.to_string())?;
    let first = identity(IdentityInput {
        owner: "acme",
        repository: "widget",
        target_repository_id: 7001,
        session_id: "session-a",
        demand_id: 12,
        message_id: None,
        sample,
        ordinal: 0,
    })
    .map_err(|error| error.to_string())?;
    let second = identity(IdentityInput {
        ordinal: 1,
        ..IdentityInput {
            owner: "acme",
            repository: "widget",
            target_repository_id: 7001,
            session_id: "session-a",
            demand_id: 12,
            message_id: None,
            sample,
            ordinal: 0,
        }
    })
    .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let CapacityClaim::New(id) = journal
        .reserve_assigned_launch_if_accepting(&first, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first assigned ordinal was not reserved".to_owned());
    };
    assert_eq!(
        journal
            .reserve_assigned_launch_if_accepting(&second, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 1,
            maximum,
        }
    );
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_assigned_launch_if_accepting(&second, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::CapacityFull {
            occupied: 1,
            maximum,
        }
    );
    assert_eq!(
        journal
            .reserve_assigned_launch_if_accepting(&first, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Existing(id)
    );
    Ok(())
}

#[tokio::test]
async fn changed_repository_id_or_session_gets_a_distinct_stable_slot() -> Result<(), String> {
    let scratch = Scratch::new("assigned-identity-scope").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let sample = observation(1_800_000_000_000, 3, 1).map_err(|error| error.to_string())?;
    let first = identity(IdentityInput {
        owner: "acme",
        repository: "widget",
        target_repository_id: 7001,
        session_id: "session-a",
        demand_id: 2,
        message_id: None,
        sample,
        ordinal: 0,
    })
    .map_err(|error| error.to_string())?;
    let different_repo = identity(IdentityInput {
        target_repository_id: 7002,
        ..IdentityInput {
            owner: "acme",
            repository: "widget",
            target_repository_id: 7001,
            session_id: "session-a",
            demand_id: 2,
            message_id: None,
            sample,
            ordinal: 0,
        }
    })
    .map_err(|error| error.to_string())?;
    let different_session = identity(IdentityInput {
        session_id: "session-b",
        ..IdentityInput {
            owner: "acme",
            repository: "widget",
            target_repository_id: 7001,
            session_id: "session-a",
            demand_id: 2,
            message_id: None,
            sample,
            ordinal: 0,
        }
    })
    .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(3).ok_or("nonzero maximum")?;
    assert!(matches!(
        journal
            .reserve_assigned_launch_if_accepting(&first, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    assert!(matches!(
        journal
            .reserve_assigned_launch_if_accepting(&different_repo, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    assert!(matches!(
        journal
            .reserve_assigned_launch_if_accepting(&different_session, maximum)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    Ok(())
}

#[tokio::test]
async fn concurrent_replay_with_changed_audit_sample_creates_one_row() -> Result<(), String> {
    let scratch = Scratch::new("assigned-concurrent-replay").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first_journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second_journal = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let first_sample = observation(1_800_000_000_000, 5, 2).map_err(|error| error.to_string())?;
    let second_sample = observation(1_800_000_000_001, 6, 2).map_err(|error| error.to_string())?;
    let first = identity(IdentityInput {
        owner: "acme",
        repository: "widget",
        target_repository_id: 7001,
        session_id: "session-a",
        demand_id: 12,
        message_id: Some(19),
        sample: first_sample,
        ordinal: 1,
    })
    .map_err(|error| error.to_string())?;
    let second = identity(IdentityInput {
        sample: second_sample,
        ..IdentityInput {
            owner: "acme",
            repository: "widget",
            target_repository_id: 7001,
            session_id: "session-a",
            demand_id: 12,
            message_id: Some(19),
            sample: first_sample,
            ordinal: 1,
        }
    })
    .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let a = async {
        barrier.clone().wait().await;
        first_journal
            .reserve_assigned_launch_if_accepting(&first, maximum)
            .await
    };
    let b = async {
        barrier.clone().wait().await;
        second_journal
            .reserve_assigned_launch_if_accepting(&second, maximum)
            .await
    };
    let (a, b, _) = tokio::join!(a, b, barrier.wait());
    let a = a.map_err(|error| error.to_string())?;
    let b = b.map_err(|error| error.to_string())?;
    let ids = match (a, b) {
        (CapacityClaim::New(a), CapacityClaim::Existing(b))
        | (CapacityClaim::Existing(a), CapacityClaim::New(b))
            if a == b =>
        {
            a
        }
        pair => return Err(format!("concurrent slot claims differ: {pair:?}")),
    };
    let observer = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    let rows = observer.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, ids);
    Ok(())
}

#[test]
fn observation_and_route_validation_are_fail_closed() {
    assert_eq!(
        observation(1, 1, 1),
        Err(HostError::Journal),
        "zero assigned demand cannot authorize a slot"
    );
    assert_eq!(
        observation(0, 2, 1),
        Err(HostError::Journal),
        "zero observation time is not a fresh sample"
    );
    let sample = observation(1_800_000_000_000, 2, 1).expect("valid sample");
    assert_eq!(
        identity(IdentityInput {
            owner: "acme",
            repository: "widget",
            target_repository_id: 0,
            session_id: "session-a",
            demand_id: 1,
            message_id: None,
            sample,
            ordinal: 0,
        }),
        Err(HostError::Journal)
    );
}
