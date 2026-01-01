//! Assigned-population launch. No acquire.

use std::sync::{Arc, Mutex};

use crate::launch::fakes::valid_worker_volume;
use crate::launch::harness::{CANARY, Mode, Script, absent, assigned_wait, ctx, open};
use crate::launch::{drive_offer, drive_offer_tracked};
use velnor_runner_host::{EnsureError, HostError, IntentState, Outcome, Started};

#[tokio::test]
async fn scale_mints_jit_then_acks_without_acquire() -> Result<(), String> {
    let (scratch, journal) = open("scale").await?;
    let volume = Arc::new(Mutex::new(String::new()));
    let seen = Arc::clone(&volume);
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let outcome = drive_offer_tracked(
        &mut script,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |name, jit, _bind| {
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
    assert_eq!(outcome.acknowledged_message_id, Some(7));
    let started = outcome.started.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(started.runner_id, "runner-1");
    assert_eq!(script.calls, ["jit", "ack"]);
    {
        let slot = volume.lock().map_err(|err| err.to_string())?;
        assert!(valid_worker_volume(slot.as_str()));
    }
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    absent(&scratch.file())
}

#[tokio::test]
async fn idless_uncertain_subject_keeps_its_reservation() -> Result<(), String> {
    let (scratch, journal) = open("idless-uncertain").await?;
    let row = journal
        .begin("launch", "m9")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let result = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(9, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(result, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, Vec::<&'static str>::new());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "m9");
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}

#[tokio::test]
async fn partial_uncertain_subject_does_not_mint() -> Result<(), String> {
    let (scratch, journal) = open("partial-mint").await?;
    let stale = journal
        .begin("launch", "m9")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(stale, None, Some("dind-kept"))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(stale, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(9, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, Vec::<&'static str>::new());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-kept"));
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
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Uncertain));
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
        |_name, _jit, _bind| async {
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
    let again = drive_offer_tracked(
        &mut second,
        &ctx(),
        &assigned_wait(8, 1),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: "dind-2".to_owned(),
                runner_id: "runner-2".to_owned(),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(
        again.started.as_ref().map(|item| item.runner_id.as_str()),
        Some("runner-2")
    );
    assert_eq!(again.acknowledged_message_id, Some(8));
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
        |_name, _jit, _bind| async {
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
    let again = drive_offer_tracked(
        &mut second,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(again.started, None);
    assert_eq!(again.acknowledged_message_id, Some(7));
    assert_eq!(second.calls, ["ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    absent(&scratch.file())
}

#[tokio::test]
async fn a_new_statistics_subject_does_not_reuse_a_bound_uncertain_row() -> Result<(), String> {
    let (scratch, journal) = open("scale-cross-subject").await?;
    let old = journal
        .begin("launch", "m7")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(old, Some("runner-old"), None)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(old, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let old_before = journal.rows().await.map_err(|error| error.to_string())?[0].clone();
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };

    let started = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(8, 1),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: "dind-new".to_owned(),
                runner_id: "runner-new".to_owned(),
            })
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(
        started.map(|item| item.runner_id).as_deref(),
        Some("runner-new")
    );
    assert_eq!(script.calls, ["jit", "ack"]);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], old_before);
    assert_eq!(rows[1].subject, "m8");
    assert_eq!(rows[1].state, IntentState::Done);
    assert_eq!(rows[1].docker_id.as_deref(), Some("runner-new"));
    absent(&scratch.file())
}

#[tokio::test]
async fn a_failed_new_statistics_subject_does_not_ack_or_change_a_pending_row() -> Result<(), String>
{
    let (scratch, journal) = open("scale-cross-subject-failure").await?;
    let old = journal
        .begin("launch", "m7")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(old, Some("runner-old"), None)
        .await
        .map_err(|error| error.to_string())?;
    let old_before = journal.rows().await.map_err(|error| error.to_string())?[0].clone();
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitFail,
    };

    let error = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(8, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;

    assert_eq!(error, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, ["jit"]);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], old_before);
    assert_eq!(rows[0].state, IntentState::Pending);
    assert_eq!(rows[1].subject, "m8");
    assert_eq!(rows[1].state, IntentState::Uncertain);
    assert_eq!(rows[1].docker_id, None);
    absent(&scratch.file())
}
