//! Engine-bound reservation and launch regressions.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use velnor_runner_host::stage::{PairSink, RunnerStartRequirement};
use velnor_runner_journal::journal::{AssignedPopulationObservation, JournalDockerDaemonBinding};

use super::super::{
    AssignedSlotIdentity, LaunchEffectOutcome, ReserveOutcome, reserve_assigned_identity,
    reserve_available_identity, run_reserved_launch,
};
use super::{daemon_binding, open_journal, route};
use crate::linux::worker::JournalPairSink;

async fn assert_daemon_binding(
    journal: &velnor_runner_host::Journal,
    launch_id: i64,
    expected: &JournalDockerDaemonBinding,
) -> Result<(), String> {
    let actual = journal
        .launch_daemon_binding(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    if actual.as_ref() == Some(expected) {
        Ok(())
    } else {
        Err("launch daemon binding did not persist".to_owned())
    }
}

async fn assert_completed_available_launch(
    journal: &velnor_runner_host::Journal,
    launch_id: i64,
    binding: &JournalDockerDaemonBinding,
) -> Result<(), String> {
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, launch_id);
    assert_daemon_binding(journal, row.id, binding).await?;
    assert_eq!(row.docker_id.as_deref(), Some("c1d2e3f4"));
    assert_eq!(row.dind_id.as_deref(), Some("b1c2d3e4"));
    assert_eq!(row.outer_network_id.as_deref(), Some("a1b2c3d4"));
    assert_eq!(row.worker_volume.as_deref(), Some("worker-coordinator"));
    assert_eq!(row.runner_request_id, Some(34));
    assert_eq!(row.requested_workflow_run_id, Some(55));
    assert_eq!(row.state, velnor_runner_host::IntentState::Done);
    Ok(())
}

#[tokio::test]
async fn available_script_persists_pair_ids_before_ack_boundary() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-coordinator-available").await?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let binding = daemon_binding("engine-a")?;
    let (reserved, launch) = reserve_available_identity(
        &journal,
        route(),
        "session-coordinator",
        &binding,
        21,
        34,
        maximum,
    )
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
    assert_completed_available_launch(&journal, launch.id, &binding).await?;
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn available_replay_on_changed_engine_cannot_authorize_another_effect() -> Result<(), String>
{
    let (scratch, journal) = open_journal("linux-coordinator-engine-replay").await?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let original = daemon_binding("engine-a")?;
    let replacement = daemon_binding("engine-b")?;
    let (reserved, launch) = reserve_available_identity(
        &journal,
        route(),
        "session-same-offer",
        &original,
        27,
        48,
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(reserved, ReserveOutcome::Reserved);
    let launch = launch.ok_or("first reservation omitted launch")?;
    let first_id = launch.id;

    let (replay, launch) = reserve_available_identity(
        &journal,
        route(),
        "session-same-offer",
        &replacement,
        27,
        48,
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(replay, ReserveOutcome::Existing);
    assert!(launch.is_none());
    assert_eq!(
        journal
            .launch_daemon_binding(first_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(original)
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

#[tokio::test]
async fn assigned_script_reserves_generic_slot_without_request_affinity() -> Result<(), String> {
    let (scratch, journal) = open_journal("linux-coordinator-assigned").await?;
    let maximum = NonZeroU32::new(1).ok_or("nonzero maximum")?;
    let binding = daemon_binding("engine-a")?;
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
        &binding,
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
    assert_daemon_binding(&journal, rows[0].id, &binding).await?;
    assert_eq!(rows[0].runner_request_id, None);
    assert_eq!(rows[0].requested_workflow_run_id, None);
    assert_eq!(rows[0].requested_job_id, None);
    drop(journal);
    drop(scratch);
    Ok(())
}
