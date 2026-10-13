//! New-subject admission keeps older uncertain reservations unchanged.

use crate::launch::drive_offer;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open};
use crate::{EnsureError, HostError, IntentState, Outcome, Started};

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

    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "session",
        })
    );
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
