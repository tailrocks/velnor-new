//! Statistics mint uses one subject per session runner.

use crate::launch_harness::{Mode, Script, absent, ctx, open};
use crate::{IntentState, Journal, Started};

use super::steps::scale_unacked;

async fn mint(
    script: &mut Script,
    journal: &Journal,
    subject: &str,
    dind: &str,
    runner: &str,
    starts: &std::cell::Cell<i32>,
) -> Result<Option<Started>, String> {
    scale_unacked(script, &ctx(), journal, subject, |_name, _jit, _bind| {
        starts.set(starts.get() + 1);
        async {
            Ok(Started {
                dind_id: dind.to_owned(),
                runner_id: runner.to_owned(),
            })
        }
    })
    .await
    .map_err(|err| err.to_string())
}

#[tokio::test]
async fn a_second_statistics_name_mints_again() -> Result<(), String> {
    let (scratch, journal) = open("scale-subject").await?;
    let first_starts = std::cell::Cell::new(0);
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = mint(
        &mut first,
        &journal,
        "sone1",
        "dind-1",
        "runner-1",
        &first_starts,
    )
    .await?;
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
    let again = mint(
        &mut second,
        &journal,
        "stwo2",
        "dind-2",
        "runner-2",
        &second_starts,
    )
    .await?;
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
    let repeated = mint(
        &mut replay,
        &journal,
        "sone1",
        "dind-3",
        "runner-3",
        &replay_starts,
    )
    .await?;
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
