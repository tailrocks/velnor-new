//! Assignment reservations participate in the production poll admission gate.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use velnor_runner_github::Poll;

use crate::HostError;
use crate::Outcome;
use crate::journal::LaunchReservation;
use crate::launch::inspect_tests::DockerStub;
use crate::launch::{Admit, drive_offer_reserved};
use crate::launch_harness::{Mode, Script, available, ctx, open, prepare};

use super::admission;

#[tokio::test]
async fn full_capacity_replay_of_existing_assignment_is_admitted() -> Result<(), String> {
    let (_scratch, journal) = open("admission-existing").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    journal
        .bind_pair(id, "runner-42", "dind-42")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(101, 42))
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(admission.reservation, Some(LaunchReservation::Existing(id)));
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn admitted_full_capacity_replay_uses_existing_worker_and_only_acks() -> Result<(), String> {
    let (_scratch, journal) = open("admission-dispatch").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    journal
        .bind_pair(id, "runner-42", "dind-42")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let polled = batch(101, 42);
    let admission = admission(&journal, 1, 1, 1, 0, 0, &polled)
        .await
        .map_err(|error| error.to_string())?;
    let prepare_called = Arc::new(AtomicBool::new(false));
    let start_called = Arc::new(AtomicBool::new(false));
    let prepare_flag = Arc::clone(&prepare_called);
    let start_flag = Arc::clone(&start_called);
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let result = drive_offer_reserved(
        &mut script,
        &ctx(),
        &polled,
        &journal,
        admission.reservation,
        |identity| {
            prepare_flag.store(true, Ordering::Relaxed);
            prepare(identity)
        },
        |_identity, _prepared, _jit| {
            start_flag.store(true, Ordering::Relaxed);
            async { Err(HostError::Docker) }
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(result, None);
    assert_eq!(script.calls, ["ack"]);
    assert!(!prepare_called.load(Ordering::Relaxed));
    assert!(!start_called.load(Ordering::Relaxed));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].state, crate::IntentState::Done);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-42"));
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-42"));
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn free_capacity_creates_and_admits_a_new_assignment() -> Result<(), String> {
    let (_scratch, journal) = open("admission-new").await?;

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(100, 42))
        .await
        .map_err(|error| error.to_string())?;

    let Some(LaunchReservation::New(id)) = admission.reservation else {
        return Err("expected a new assignment reservation".to_owned());
    };
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    assert_eq!(journal.rows().await.map_err(|e| e.to_string())?[0].id, id);
    Ok(())
}

#[tokio::test]
async fn full_capacity_rejects_an_unrelated_assignment() -> Result<(), String> {
    let (_scratch, journal) = open("admission-unrelated").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected an occupying assignment".to_owned());
    };

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(101, 43))
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(admission.reservation, Some(LaunchReservation::AtCapacity));
    assert_eq!(admission.decision, Admit::Hold);
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    assert_eq!(journal.rows().await.map_err(|e| e.to_string())?.len(), 1);
    assert_eq!(
        journal.intent(id).await.map_err(|e| e.to_string())?.subject,
        "s1:42"
    );
    Ok(())
}

#[tokio::test]
async fn completed_assignment_replay_is_acked_without_a_new_reservation() -> Result<(), String> {
    let (_scratch, journal) = open("admission-completed-replay").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the first acquire claim".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the first JIT claim".to_owned());
    }
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let name = format!("v{}", identity.launch_id());
    journal
        .record_runner_completed(1, 42, 71, &name)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup(id)
        .await
        .map_err(|error| error.to_string())?;

    let replay = admission(&journal, 1, 1, 1, 1, 0, &batch(101, 42))
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(replay.reservation, Some(LaunchReservation::Completed(id)));
    assert_eq!(replay.decision, Admit::Ack { stop: false });
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    Ok(())
}

#[tokio::test]
async fn unresolved_assignment_reservation_blocks_initial_scale_mint() -> Result<(), String> {
    let (_scratch, journal) = open("scale-reserved-assignment").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 2)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let docker = DockerStub::open(Vec::new())?;
    let mut scaled = false;

    let result = super::scale_if_free(&journal, &docker.docker, 2, 1, || {
        scaled = true;
        async { Ok(None) }
    })
    .await
    .map_err(|error| error.to_string())?;
    docker.finish().await?;

    assert_eq!(result, None);
    assert!(
        !scaled,
        "occupied assignment reservation must block scaling"
    );
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    Ok(())
}

fn batch(message_id: i64, request_id: i64) -> Poll {
    let mut polled = available(&[request_id]);
    if let Poll::Batch(batch) = &mut polled {
        batch.message_id = message_id;
    }
    polled
}
