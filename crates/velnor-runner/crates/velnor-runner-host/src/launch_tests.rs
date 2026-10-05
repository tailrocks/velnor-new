//! Launch ordering. The scripted transport is the acquire, JIT, and ack path.

use std::sync::{Arc, Mutex};

use velnor_runner_github::{ParsedBatch, Poll};

use crate::journal::Outcome;
use crate::launch::{Idle, drive_offer, fail_unstarted, idle};
use crate::launch_harness::{CANARY, Mode, Script, absent, assigned_wait, available, ctx, open};
use crate::launch_test_support::valid_worker_volume;
use crate::{EnsureError, HostError, IntentState, Started};

#[test]
fn statistics_advance_and_offers_stay() {
    let stats = Poll::Batch(ParsedBatch {
        message_id: 2,
        statistics: None,
        jobs: Vec::new(),
    });
    assert_eq!(idle(&stats), Idle::Ack);
    assert_eq!(idle(&available(&[3])), Idle::Launch);
    assert_eq!(idle(&Poll::Empty), Idle::Empty);
    assert_eq!(idle(&available(&[3, 4])), Idle::Blocked);
    assert_eq!(idle(&assigned_wait(7, 1)), Idle::Scale);
    assert_eq!(idle(&assigned_wait(7, 0)), Idle::Ack);
}

#[tokio::test]
async fn name_taken_failure_releases_the_unstarted_scale_row() -> Result<(), String> {
    let (scratch, journal) = open("name-taken").await?;
    let id = journal
        .begin("launch", "m100000769")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let failed = fail_unstarted(&journal, &assigned_wait(100_000_769, 5))
        .await
        .map_err(|err| err.to_string())?;
    if !failed {
        return Err("unstarted row was not failed".to_owned());
    }
    let state = journal.read(id).await.map_err(|err| err.to_string())?;
    if state != IntentState::Failed {
        return Err(format!("state {state:?}"));
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn name_taken_keeps_a_row_that_has_a_container() -> Result<(), String> {
    let (scratch, journal) = open("name-taken-live").await?;
    let id = journal
        .begin("launch", "m100000769")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some("runner-container"), None)
        .await
        .map_err(|err| err.to_string())?;
    let failed = fail_unstarted(&journal, &assigned_wait(100_000_769, 5))
        .await
        .map_err(|err| err.to_string())?;
    if failed {
        return Err("container row was treated as a name collision".to_owned());
    }
    let state = journal.read(id).await.map_err(|err| err.to_string())?;
    if state != IntentState::Uncertain {
        return Err(format!("state {state:?}"));
    }
    absent(&scratch.file())
}

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
    assert!(replay.calls.is_empty());
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
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "session",
        })
    );
    assert_eq!(script.calls, ["acquire", "jit", "ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-1"));
    absent(&scratch.file())
}
