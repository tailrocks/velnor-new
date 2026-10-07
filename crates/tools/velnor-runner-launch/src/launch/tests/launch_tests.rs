//! Launch ordering. The scripted transport is the acquire, JIT, and ack path.

use std::sync::{Arc, Mutex};

use crate::launch::drive_offer;
use crate::launch::fakes::valid_worker_volume;
use crate::launch::harness::{CANARY, Mode, Script, absent, available, ctx, open};
use velnor_runner_host::{EnsureError, HostError, IntentState, Started};

#[tokio::test]
async fn launch_acks_only_after_start_and_hides_jit() -> Result<(), String> {
    let (scratch, journal) = open("ok").await?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |volume, jit, _bind| {
            let volume = volume.to_owned();
            let jit = jit.to_vec();
            let captured = Arc::clone(&captured);
            async move {
                if !valid_worker_volume(&volume) {
                    return Err(HostError::ForbiddenMount);
                }
                let mut slot = captured.lock().map_err(|_| HostError::Docker)?;
                *slot = jit;
                Ok(Started {
                    dind_id: "dind-1".to_owned(),
                    runner_id: "runner-1".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let started = started.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(started.runner_id, "runner-1");
    assert_eq!(script.calls, ["acquire", "jit", "ack"]);
    {
        let slot = seen.lock().map_err(|err| err.to_string())?;
        assert_eq!(slot.as_slice(), CANARY.as_bytes());
    }
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-1"));
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-1"));
    assert_eq!(rows[0].message_id, Some(4));
    assert_eq!(rows[0].runner_request_id, Some(3));
    assert_eq!(rows[0].runner_name.as_deref(), Some("g2_1"));
    absent(&scratch.file())
}

#[tokio::test]
async fn uncertain_acquire_does_not_ack() -> Result<(), String> {
    let (scratch, journal) = open("timeout").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Timeout,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await
    .map_err(|err| err.to_string());
    assert_eq!(error, Err("effect uncertain".to_owned()));
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id, None);
    let id = rows[0].id;
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let replayed = drive_offer(
        &mut replay,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(replayed, Err(EnsureError::Uncertain));
    assert_eq!(replay.calls, Vec::<&'static str>::new());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn forbidden_acquire_is_failed_and_not_acked() -> Result<(), String> {
    let (_scratch, journal) = open("forbidden").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Forbidden,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Forbidden));
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Failed);
    Ok(())
}

#[tokio::test]
async fn empty_acquire_is_not_acked() -> Result<(), String> {
    let (_scratch, journal) = open("empty").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Empty,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "acquire",
        })
    );
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Failed);
    Ok(())
}

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
    assert_eq!(
        rows,
        Vec::<velnor_runner_journal::reconcile::IntentRow>::new()
    );
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
