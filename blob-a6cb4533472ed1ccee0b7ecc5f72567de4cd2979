//! `drive_offer` acquire/start/ack sequencing and replay.

use crate::launch::drive_offer;
use crate::launch_harness::{Mode, Script, absent, available, ctx, open};

use super::super::{EnsureError, HostError, IntentState, Started};

#[tokio::test]
async fn two_offers_are_not_acquired() -> Result<(), String> {
    let (_scratch, journal) = open("two").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3, 4]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        })
    );
    assert_eq!(script.calls, Vec::<&str>::new());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows, Vec::new());
    Ok(())
}

#[tokio::test]
async fn bound_runner_acks_without_a_second_start() -> Result<(), String> {
    let (scratch, journal) = open("replay").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut first,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async {
            Ok(Started {
                dind_id: "dind-1".to_owned(),
                runner_id: "runner-1".to_owned(),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(
        started.map(|item| item.runner_id).as_deref(),
        Some("runner-1")
    );
    let mut second = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let again = drive_offer(
        &mut second,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(again, None);
    assert_eq!(second.calls, ["ack"]);
    absent(&scratch.file())
}

#[tokio::test]
async fn start_failure_after_acquire_is_not_acked() -> Result<(), String> {
    let (scratch, journal) = open("start").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, ["acquire", "jit"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id, None);
    absent(&scratch.file())
}

#[tokio::test]
async fn jit_uncertainty_does_not_repeat_acquire_or_jit() -> Result<(), String> {
    let (_scratch, journal) = open("jit-once").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::JitFail,
    };
    let first_result = drive_offer(
        &mut first,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(first_result, Err(EnsureError::Uncertain));
    assert_eq!(first.calls, ["acquire", "jit"]);

    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    assert_eq!(
        drive_offer(
            &mut replay,
            &ctx(),
            &available(&[3]),
            &journal,
            |_volume, _jit, _bind| async { Err(HostError::Docker) },
        )
        .await,
        Err(EnsureError::Uncertain)
    );
    assert!(replay.calls.is_empty());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn ack_failure_keeps_the_runner_bound() -> Result<(), String> {
    let (scratch, journal) = open("ack").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::AckFail,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async {
            Ok(Started {
                dind_id: "dind-1".to_owned(),
                runner_id: "runner-1".to_owned(),
            })
        },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, ["acquire", "jit", "ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-1"));
    absent(&scratch.file())
}
