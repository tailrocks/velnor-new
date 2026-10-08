//! Journal-backed regressions for the durable launch effect boundary.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use velnor_runner_host::Journal;
use velnor_runner_host::stage::{PairSink, RunnerStartRequirement};
use velnor_runner_journal::journal::{
    AssignedPopulationObservation, CapacityClaim, ReplayRoute, RunnerStartIntent,
    ScopedLaunchIdentity,
};

use super::{AssignedSlotIdentity, LaunchEffectOutcome, ReservedLaunch, run_reserved_launch};
use super::{ReserveOutcome, reserve_assigned_identity, reserve_available_identity};
use crate::launch::harness::Scratch;
use crate::linux::worker::JournalPairSink;

fn route() -> ReplayRoute<'static> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner: "acme",
        repository: "widget",
        runner_group_id: 7,
        runner_group_name: "private",
        scale_set_id: 9,
        scale_set_name: "ubuntu-26.04-scale-set",
    }
}

async fn open_journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}

#[tokio::test]
async fn reserve_persists_before_effect_and_marks_success_done() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-capacity-success").await?;
    let identity = ScopedLaunchIdentity::new(route(), "session-a", 4, 12)
        .map_err(|error| error.to_string())?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&identity, NonZeroU32::new(1).ok_or("nonzero maximum")?)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("new offer did not reserve capacity".to_owned());
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let result = run_reserved_launch(
        &journal,
        &ReservedLaunch {
            id,
            runner_name: format!("v{id:x}"),
        },
        Some(4),
        Some(12),
        Some(91),
        Some("opaque-scale-set-job"),
        move |_runner| async move {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(result, LaunchEffectOutcome::Done);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, velnor_runner_host::IntentState::Done);
    assert_eq!(rows[0].message_id, Some(4));
    assert_eq!(rows[0].runner_request_id, Some(12));
    assert_eq!(rows[0].requested_workflow_run_id, Some(91));
    assert_eq!(
        rows[0].requested_job_id.as_deref(),
        Some("opaque-scale-set-job")
    );
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn uncertain_effect_stays_occupied_and_cannot_be_replayed() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-capacity-uncertain").await?;
    let identity = ScopedLaunchIdentity::new(route(), "session-b", 5, 13)
        .map_err(|error| error.to_string())?;
    let CapacityClaim::New(id) = journal
        .reserve_launch_if_accepting(&identity, NonZeroU32::new(1).ok_or("nonzero maximum")?)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("new offer did not reserve capacity".to_owned());
    };
    let launch = ReservedLaunch {
        id,
        runner_name: format!("v{id:x}"),
    };
    assert_eq!(
        run_reserved_launch(
            &journal,
            &launch,
            Some(5),
            Some(13),
            Some(92),
            Some("opaque-scale-set-job-b"),
            |_runner| async { Err(velnor_runner_host::HostError::Docker) },
        )
        .await
        .map_err(|error| error.to_string())?,
        LaunchEffectOutcome::Uncertain
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    assert_eq!(
        run_reserved_launch(
            &journal,
            &launch,
            Some(5),
            Some(13),
            Some(92),
            Some("opaque-scale-set-job-b"),
            move |_runner| async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .map_err(|error| error.to_string())?,
        LaunchEffectOutcome::Uncertain
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, velnor_runner_host::IntentState::Uncertain);
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn available_script_persists_pair_ids_before_ack_boundary() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-coordinator-available").await?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let (reserved, launch) =
        reserve_available_identity(&journal, route(), "session-coordinator", 21, 34, maximum)
            .await
            .map_err(|error| error.to_string())?;
    assert_eq!(reserved, ReserveOutcome::Reserved);
    let launch = launch.ok_or("reservation omitted launch row")?;
    let sink = JournalPairSink {
        journal: &journal,
        launch_id: launch.id,
    };
    let calls = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let observed = Arc::clone(&calls);
    let outcome = run_reserved_launch(
        &journal,
        &launch,
        Some(21),
        Some(34),
        Some(55),
        Some("opaque-scale-set-job"),
        move |_runner| async move {
            observed
                .lock()
                .map_err(|_| velnor_runner_host::HostError::Journal)?
                .push("acquire");
            observed
                .lock()
                .map_err(|_| velnor_runner_host::HostError::Journal)?
                .push("jit");
            sink.volume("worker-coordinator")
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            sink.outer_network_intent("worker-coordinator-outer")
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            sink.outer_network("a1b2c3d4")
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            sink.dind("b1c2d3e4")
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            sink.runner("c1d2e3f4")
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            sink.before_runner_start("c1d2e3f4", RunnerStartRequirement::DurableRequired)
                .await
                .map_err(|_| velnor_runner_host::HostError::Journal)?;
            observed
                .lock()
                .map_err(|_| velnor_runner_host::HostError::Journal)?
                .push("start");
            Ok(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(outcome, LaunchEffectOutcome::Done);
    assert_eq!(
        calls.lock().map_err(|_| "script log poisoned")?.as_slice(),
        &["acquire", "jit", "start"]
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.docker_id.as_deref(), Some("c1d2e3f4"));
    assert_eq!(row.dind_id.as_deref(), Some("b1c2d3e4"));
    assert_eq!(row.outer_network_id.as_deref(), Some("a1b2c3d4"));
    assert_eq!(row.worker_volume.as_deref(), Some("worker-coordinator"));
    assert_eq!(row.runner_request_id, Some(34));
    assert_eq!(row.requested_workflow_run_id, Some(55));
    assert_eq!(row.state, velnor_runner_host::IntentState::Done);
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn assigned_script_reserves_generic_slot_without_request_affinity() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-coordinator-assigned").await?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let observation = AssignedPopulationObservation::new(1_800_000_000_000, 2, 0)
        .map_err(|error| error.to_string())?;
    let (reserved, launch) = reserve_assigned_identity(
        &journal,
        AssignedSlotIdentity {
            route: route(),
            target_repository_id: 1234,
            session_id: "session-coordinator",
            demand_id: 77,
            message_id: Some(22),
            observed_ms: u128::from(observation.observed_at_ms()),
            assigned_jobs: observation.assigned_jobs(),
            running_jobs: observation.running_jobs(),
            ordinal: 0,
        },
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(reserved, ReserveOutcome::Reserved);
    let launch = launch.ok_or("assigned reservation omitted launch row")?;
    let jit_count = Arc::new(AtomicUsize::new(0));
    let observed_jit = Arc::clone(&jit_count);
    assert_eq!(
        run_reserved_launch(
            &journal,
            &launch,
            Some(22),
            None,
            None,
            None,
            move |_runner| async move {
                observed_jit.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .map_err(|error| error.to_string())?,
        LaunchEffectOutcome::Done
    );
    assert_eq!(jit_count.load(Ordering::SeqCst), 1);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].runner_request_id, None);
    assert_eq!(rows[0].requested_workflow_run_id, None);
    assert_eq!(rows[0].requested_job_id, None);
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn pair_sink_refuses_runner_start_after_drain_fence() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-pair-start-drain-fence").await?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let (reserved, launch) =
        reserve_available_identity(&journal, route(), "session-drain", 31, 41, maximum)
            .await
            .map_err(|error| error.to_string())?;
    assert_eq!(reserved, ReserveOutcome::Reserved);
    let launch = launch.ok_or("reservation omitted launch row")?;
    journal
        .bind_launch_identity(
            launch.id,
            Some(31),
            Some(41),
            Some(51),
            Some("opaque-job"),
            &launch.runner_name,
        )
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch.id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(launch.id, Some("c1d2e3f4"), None)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;

    let sink = JournalPairSink {
        journal: &journal,
        launch_id: launch.id,
    };
    assert_eq!(
        sink.before_runner_start("c1d2e3f4", RunnerStartRequirement::DurableRequired)
            .await,
        Err(velnor_runner_host::HostError::Journal)
    );
    assert_eq!(
        journal
            .runner_start_intent(launch.id)
            .await
            .map_err(|error| error.to_string())?,
        RunnerStartIntent::NotRequested
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    drop(journal);
    drop(scratch);
    Ok(())
}
