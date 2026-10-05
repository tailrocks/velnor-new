//! A partial scale row stays occupied until cleanup, not a second JIT.

use crate::launch::drive_offer;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open};
use crate::{EnsureError, HostError, IntentState};

#[tokio::test]
async fn partial_scale_row_is_not_retired() -> Result<(), String> {
    let (scratch, journal) = open("scale-partial").await?;
    let (id, _) = journal
        .begin_launch("m7")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker_volume(id, "wpartial")
        .await
        .map_err(|err| err.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
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
    assert_eq!(script.calls.len(), 0);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_ne!(rows[0].state, IntentState::Failed);
    assert_eq!(rows[0].worker_volume.as_deref(), Some("wpartial"));
    absent(&scratch.file())
}
