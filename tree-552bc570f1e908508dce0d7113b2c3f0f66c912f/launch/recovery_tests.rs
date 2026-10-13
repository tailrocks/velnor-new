//! Assignment recovery after an uncertain acknowledgement or acquire.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use velnor_runner_github::Poll;

use crate::journal::LaunchReservation;
use crate::launch::steps_acquire;
use crate::launch_harness::{Mode, Script, available, ctx, open, prepare};
use crate::{EnsureError, HostError, IntentState, Outcome, Started};

#[tokio::test]
async fn bound_pair_redelivery_at_full_capacity_retries_only_ack() -> Result<(), String> {
    let (_scratch, journal) = open("bound-recovery").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::AckFail,
    };
    let failed = steps_acquire::launch_id(
        &mut first,
        &ctx(),
        &batch(100, 42),
        &journal,
        42,
        prepare,
        |_identity, prepared, _jit| {
            let dind_id = prepared.dind_id().to_owned();
            async move {
                Ok(Started {
                    dind_id,
                    runner_id: "runner-42".to_owned(),
                })
            }
        },
    )
    .await;

    assert_eq!(
        failed,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "session",
        })
    );
    assert_eq!(first.calls, ["acquire", "jit", "ack"]);
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|err| err.to_string())?,
        1
    );
    let before = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].state, IntentState::Uncertain);
    assert_eq!(before[0].docker_id.as_deref(), Some("runner-42"));
    assert!(before[0].dind_id.is_some());

    let prepare_called = Arc::new(AtomicBool::new(false));
    let start_called = Arc::new(AtomicBool::new(false));
    let prepare_flag = Arc::clone(&prepare_called);
    let start_flag = Arc::clone(&start_called);
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let recovered = steps_acquire::launch_id(
        &mut replay,
        &ctx(),
        &batch(101, 42),
        &journal,
        42,
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
    .map_err(|err| err.to_string())?;

    assert_eq!(recovered, None);
    assert_eq!(replay.calls, ["ack"]);
    assert!(!prepare_called.load(Ordering::Relaxed));
    assert!(!start_called.load(Ordering::Relaxed));
    let after = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(after[0].state, IntentState::Done);
    assert_eq!(after[0].docker_id, before[0].docker_id);
    assert_eq!(after[0].dind_id, before[0].dind_id);
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|err| err.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn resolved_acquire_without_jit_resumes_without_another_acquire() -> Result<(), String> {
    let (_scratch, journal) = open("acquire-recovery").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|err| err.to_string())?
    {
        return Err("expected to claim the first acquire".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;

    let mut resumed = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = steps_acquire::launch_id(
        &mut resumed,
        &ctx(),
        &batch(101, 42),
        &journal,
        42,
        prepare,
        |_identity, prepared, _jit| {
            let dind_id = prepared.dind_id().to_owned();
            async move {
                Ok(Started {
                    dind_id,
                    runner_id: "runner-42".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;

    assert_eq!(
        started.map(|item| item.runner_id).as_deref(),
        Some("runner-42")
    );
    assert_eq!(resumed.calls, ["jit", "ack"]);
    let row = journal.intent(id).await.map_err(|err| err.to_string())?;
    assert!(row.acquire_attempted);
    assert!(row.acquire_resolved);
    assert!(row.acquired);
    assert!(row.jit_requested);
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|err| err.to_string())?,
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
