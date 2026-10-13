//! An idless uncertain scale row fails, then the same subject mints.

use crate::launch::drive_offer;
use crate::launch_harness::{JitProbe, Mode, Script, absent, assigned_wait, ctx, open};
use crate::{EnsureError, HostError, IntentState, Outcome, Started};

#[tokio::test]
async fn idless_uncertain_subject_mints_after_the_empty_row_fails() -> Result<(), String> {
    let (scratch, journal) = open("idless-mint").await?;
    let stale = journal
        .begin("launch", "m9")
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
    let started = drive_offer(
        &mut script,
        &ctx(),
        &assigned_wait(9, 1),
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
    let started = started.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(started.runner_id, "runner-2");
    assert_eq!(script.calls, ["jit", "ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, stale);
    assert_eq!(rows[0].state, IntentState::Failed);
    assert_eq!(rows[1].state, IntentState::Done);
    assert_eq!(rows[1].docker_id.as_deref(), Some("runner-2"));
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
    assert!(script.calls.is_empty());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-kept"));
    absent(&scratch.file())
}

#[tokio::test]
async fn idless_jit_fail_redelivery_mints_again() -> Result<(), String> {
    let (scratch, journal) = open("scale-retry").await?;
    let mut failed = Script {
        calls: Vec::new(),
        mode: Mode::JitFail,
    };
    let error = drive_offer(
        &mut failed,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "session",
        })
    );
    let stale = journal.rows().await.map_err(|err| err.to_string())?;
    let stale = stale[0].id;
    let mut replay = JitProbe::ok();
    let started = drive_offer(
        &mut replay,
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
    let started = started.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(started.runner_id, "runner-1");
    assert_eq!(replay.calls(), ["jit", "ack"]);
    assert_eq!(replay.names.first().map(String::as_str), Some("m7"));
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, stale);
    assert_eq!(rows[0].state, IntentState::Failed);
    assert_eq!(rows[1].state, IntentState::Done);
    assert_eq!(rows[1].docker_id.as_deref(), Some("runner-1"));
    assert_eq!(rows[1].dind_id.as_deref(), Some("dind-1"));
    absent(&scratch.file())
}
