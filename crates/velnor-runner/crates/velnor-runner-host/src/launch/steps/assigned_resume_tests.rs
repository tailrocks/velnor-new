//! Redelivery resumes only a marker-proven unattempted effect.

use crate::launch::steps::launch_id;
use crate::launch_harness::{Scratch, assigned_wait, ctx};
use crate::launch_test_support::script;
use crate::{Journal, Started};

#[tokio::test]
async fn resolved_acquire_without_jit_resumes_one_jit_without_acquire() -> Result<(), String> {
    let (_scratch, journal) = open_journal("assigned-resume-jit").await?;
    let (id, fresh) = journal
        .begin_assigned_launch("m1r41", 1, 41, "v41")
        .await
        .map_err(|error| error_text(&error))?;
    assert!(fresh);
    assert!(
        journal
            .claim_assigned_acquire(id)
            .await
            .map_err(|error| error_text(&error))?
    );
    journal
        .record_assigned_acquire(id, true)
        .await
        .map_err(|error| error_text(&error))?;
    let batch = assigned_batch(2)?;
    let mut lane = script();

    let started = launch_id(
        &mut lane,
        &ctx(),
        &batch,
        &journal,
        41,
        |_volume: &str, _jit: &[u8], _bind: crate::launch::bind::Bind| async {
            Ok(Started {
                runner_id: format!("{:064x}", 41),
                dind_id: format!("{:064x}", 42),
            })
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(started.is_some());
    assert_eq!(lane.calls, ["jit", "ack"]);
    Ok(())
}

#[tokio::test]
async fn unresolved_acquire_does_not_replay_acquire_or_request_jit() -> Result<(), String> {
    let (_scratch, journal) = open_journal("assigned-resume-acquire").await?;
    let (id, _) = journal
        .begin_assigned_launch("m3r43", 1, 43, "v43")
        .await
        .map_err(|error| error_text(&error))?;
    assert!(
        journal
            .claim_assigned_acquire(id)
            .await
            .map_err(|error| error_text(&error))?
    );
    let batch = assigned_batch(4)?;
    let mut lane = script();

    let result = launch_id(
        &mut lane,
        &ctx(),
        &batch,
        &journal,
        43,
        |_volume: &str, _jit: &[u8], _bind: crate::launch::bind::Bind| async {
            Ok(Started {
                runner_id: format!("{:064x}", 41),
                dind_id: format!("{:064x}", 42),
            })
        },
    )
    .await;

    assert_eq!(result, Err(crate::EnsureError::Uncertain));
    assert_eq!(lane.calls, Vec::new());
    Ok(())
}

#[tokio::test]
async fn uncertain_jit_does_not_repeat_jit_or_acquire() -> Result<(), String> {
    let (_scratch, journal) = open_journal("assigned-resume-jit-uncertain").await?;
    let (id, _) = journal
        .begin_assigned_launch("m5r45", 1, 45, "v45")
        .await
        .map_err(|error| error_text(&error))?;
    assert!(
        journal
            .claim_assigned_acquire(id)
            .await
            .map_err(|error| error_text(&error))?
    );
    journal
        .record_assigned_acquire(id, true)
        .await
        .map_err(|error| error_text(&error))?;
    assert!(
        journal
            .claim_launch_jit(id)
            .await
            .map_err(|error| error_text(&error))?
    );
    let batch = assigned_batch(6)?;
    let mut lane = script();

    let result = launch_id(
        &mut lane,
        &ctx(),
        &batch,
        &journal,
        45,
        |_volume: &str, _jit: &[u8], _bind: crate::launch::bind::Bind| async {
            Ok(Started {
                runner_id: format!("{:064x}", 41),
                dind_id: format!("{:064x}", 42),
            })
        },
    )
    .await;

    assert_eq!(result, Err(crate::EnsureError::Uncertain));
    assert_eq!(lane.calls, Vec::new());
    Ok(())
}

async fn open_journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error_text(&error))?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error_text(&error))?;
    Ok((scratch, journal))
}

fn assigned_batch(message_id: i64) -> Result<velnor_runner_github::ParsedBatch, String> {
    let velnor_runner_github::Poll::Batch(batch) = assigned_wait(message_id, 1) else {
        return Err("expected assigned batch".to_owned());
    };
    Ok(batch)
}

fn error_text(error: &impl ToString) -> String {
    error.to_string()
}
