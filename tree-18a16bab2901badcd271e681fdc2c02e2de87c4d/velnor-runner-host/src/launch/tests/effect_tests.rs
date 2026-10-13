//! Acquisition and mint errors retain capacity when a remote effect is uncertain.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::launch::fakes::Engine;
use crate::launch::harness::{Mode, Script, absent, available, ctx, open};
use crate::{EnsureError, HostError, IntentState};

use super::super::{Admit, admission, drive_offer};

#[tokio::test]
async fn unusable_successes_hold_acquired_jobs_without_retry_or_ack() -> Result<(), String> {
    for (label, mode, expected, calls) in [
        (
            "acquire-malformed",
            Mode::AcquireMalformed,
            EnsureError::Uncertain,
            &["acquire"][..],
        ),
        (
            "acquire-foreign",
            Mode::AcquireForeign,
            EnsureError::Uncertain,
            &["acquire"][..],
        ),
        (
            "acquire-server-error",
            Mode::AcquireServerError,
            EnsureError::Uncertain,
            &["acquire"][..],
        ),
        (
            "jit-malformed",
            Mode::JitMalformed,
            EnsureError::Uncertain,
            &["acquire", "jit"][..],
        ),
        (
            "jit-conflict-after-acquire",
            Mode::JitConflict,
            EnsureError::Conflict,
            &["acquire", "jit"][..],
        ),
    ] {
        assert_held_after_error(label, mode, expected, calls).await?;
    }
    Ok(())
}

async fn assert_held_after_error(
    label: &str,
    mode: Mode,
    expected: EnsureError,
    expected_calls: &[&'static str],
) -> Result<(), String> {
    let (scratch, journal) = open(label).await?;
    let starts = Arc::new(AtomicUsize::new(0));
    let start_count = Arc::clone(&starts);
    let mut script = Script {
        calls: Vec::new(),
        mode,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        move |_volume, _jit, _bind| {
            let starts = Arc::clone(&start_count);
            async move {
                starts.fetch_add(1, Ordering::Relaxed);
                Err(HostError::Docker)
            }
        },
    )
    .await;
    assert_eq!(error, Err(expected), "{label}");
    assert_eq!(script.calls, expected_calls, "{label}");
    assert_eq!(starts.load(Ordering::Relaxed), 0, "{label}");
    assert_occupied_uncertain(&journal, label).await?;

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
    assert_eq!(replayed, Err(EnsureError::Uncertain), "{label}");
    assert!(replay.calls.is_empty(), "{label}");
    assert_occupied_uncertain(&journal, label).await?;
    absent(&scratch.file())
}

async fn assert_occupied_uncertain(journal: &crate::Journal, label: &str) -> Result<(), String> {
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1, "{label}");
    assert_eq!(rows[0].state, IntentState::Uncertain, "{label}");
    assert_eq!(rows[0].docker_id, None, "{label}");
    assert_eq!(rows[0].dind_id, None, "{label}");
    let decision = admission(&Engine::new(), journal, 1, 1, 0, &available(&[3]))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(decision, Admit::Hold, "{label}");
    Ok(())
}
