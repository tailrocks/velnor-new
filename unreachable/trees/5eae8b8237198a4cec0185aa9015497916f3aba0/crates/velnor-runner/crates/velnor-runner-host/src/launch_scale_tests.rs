//! Assigned-population launch. No acquire.

use std::sync::{Arc, Mutex};

use crate::launch::drive_offer;
use crate::launch_harness::{CANARY, Mode, Script, absent, assigned_wait, ctx, open};
use crate::{EnsureError, HostError, IntentState, Started};

#[tokio::test]
async fn scale_mints_jit_then_acks_without_acquire() -> Result<(), String> {
    let (scratch, journal) = open("scale").await?;
    let volume = Arc::new(Mutex::new(String::new()));
    let seen = Arc::clone(&volume);
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |name, jit| {
            let name = name.to_owned();
            let jit = jit.to_vec();
            let seen = Arc::clone(&seen);
            async move {
                let mut slot = seen.lock().map_err(|_| HostError::Docker)?;
                *slot = name;
                if jit.as_slice() != CANARY.as_bytes() {
                    return Err(HostError::Docker);
                }
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
    assert_eq!(script.calls, ["jit", "ack"]);
    {
        let slot = volume.lock().map_err(|err| err.to_string())?;
        assert_eq!(slot.as_str(), "m7");
    }
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    absent(&scratch.file())
}

#[tokio::test]
async fn scale_jit_failure_is_not_acked() -> Result<(), String> {
    let (scratch, journal) = open("scale-jit").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitFail,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "session",
        })
    );
    assert_eq!(script.calls, ["jit"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn finished_scale_row_does_not_block_the_next_message() -> Result<(), String> {
    let (scratch, journal) = open("scale-replay").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut first,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit| async {
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
        &assigned_wait(8, 1),
        &journal,
        |_name, _jit| async {
            Ok(Started {
                dind_id: "dind-2".to_owned(),
                runner_id: "runner-2".to_owned(),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(
        again.map(|item| item.runner_id).as_deref(),
        Some("runner-2")
    );
    assert_eq!(second.calls, ["jit", "ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    absent(&scratch.file())
}

#[tokio::test]
async fn redelivered_scale_row_does_not_count_as_a_worker() -> Result<(), String> {
    let (scratch, journal) = open("scale-redeliver").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut first,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit| async {
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
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit| async { Err(HostError::Docker) },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(again, None);
    assert_eq!(second.calls, ["ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    absent(&scratch.file())
}
