//! An uncertain JIT failure stays reserved when the scale offer is redelivered.

use crate::launch::drive_offer;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open};
use crate::{EnsureError, HostError, IntentState, Started};

#[tokio::test]
async fn uncertain_jit_failure_redelivery_stays_queued() -> Result<(), String> {
    let (scratch, journal) = open("scale-retry").await?;
    let mut failed = Script {
        calls: Vec::new(),
        mode: Mode::JitFail,
    };
    let first = drive_offer(
        &mut failed,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(first, Err(EnsureError::Uncertain));
    assert_eq!(failed.calls, ["jit"]);

    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let second = drive_offer(
        &mut replay,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: "unexpected-dind".to_owned(),
                runner_id: "unexpected-runner".to_owned(),
            })
        },
    )
    .await;
    assert_eq!(second, Err(EnsureError::Uncertain));
    assert!(replay.calls.is_empty());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}
