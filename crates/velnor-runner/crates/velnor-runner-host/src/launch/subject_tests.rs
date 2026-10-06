//! Statistics mint uses one subject per session runner.

use crate::launch_harness::{Mode, Script, absent, ctx, open};
use crate::{IntentState, Started};

use super::steps::scale_unacked;

#[tokio::test]
async fn a_second_statistics_name_mints_again() -> Result<(), String> {
    let (scratch, journal) = open("scale-subject").await?;
    let first_starts = std::cell::Cell::new(0);
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = scale_unacked(
        &mut first,
        &ctx(),
        &journal,
        "sone1",
        |_name, _jit, _bind| {
            first_starts.set(first_starts.get() + 1);
            async {
                Ok(Started {
                    dind_id: "dind-1".to_owned(),
                    runner_id: "runner-1".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(
        started.map(|item| item.runner_id).as_deref(),
        Some("runner-1")
    );
    assert_eq!(first.calls, ["jit"]);
    assert_eq!(first_starts.get(), 1);
    let second_starts = std::cell::Cell::new(0);
    let mut second = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let again = scale_unacked(
        &mut second,
        &ctx(),
        &journal,
        "stwo2",
        |_name, _jit, _bind| {
            second_starts.set(second_starts.get() + 1);
            async {
                Ok(Started {
                    dind_id: "dind-2".to_owned(),
                    runner_id: "runner-2".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(
        again.map(|item| item.runner_id).as_deref(),
        Some("runner-2")
    );
    assert_eq!(second.calls, ["jit"]);
    assert_eq!(second_starts.get(), 1);
    let replay_starts = std::cell::Cell::new(0);
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let repeated = scale_unacked(
        &mut replay,
        &ctx(),
        &journal,
        "sone1",
        |_name, _jit, _bind| {
            replay_starts.set(replay_starts.get() + 1);
            async {
                Ok(Started {
                    dind_id: "dind-3".to_owned(),
                    runner_id: "runner-3".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(repeated, None);
    assert!(replay.calls.is_empty());
    assert_eq!(replay_starts.get(), 0);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].subject, "sone1");
    assert_eq!(rows[1].subject, "stwo2");
    assert_eq!(rows[0].state, IntentState::Done);
    assert_eq!(rows[1].state, IntentState::Done);
    absent(&scratch.file())
}
