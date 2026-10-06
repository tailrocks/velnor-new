//! Statistics mint uses one subject per session runner.

use crate::launch::harness::{Mode, Script, absent, ctx, open};
use crate::{IntentState, Started};

use super::super::steps::scale_unacked;

#[tokio::test]
async fn a_second_statistics_name_mints_again() -> Result<(), String> {
    let (scratch, journal) = open("scale-subject").await?;
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = scale_unacked(
        &mut first,
        &ctx(),
        &journal,
        "s-one",
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
    assert_eq!(first.calls, ["jit"]);
    let mut second = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let again = scale_unacked(
        &mut second,
        &ctx(),
        &journal,
        "s-two",
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
        again.map(|item| item.runner_id).as_deref(),
        Some("runner-2")
    );
    assert_eq!(second.calls, ["jit"]);
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let repeated = scale_unacked(
        &mut replay,
        &ctx(),
        &journal,
        "s-one",
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: "dind-3".to_owned(),
                runner_id: "runner-3".to_owned(),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(repeated, None);
    assert!(replay.calls.is_empty());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].subject, "s-one");
    assert_eq!(rows[1].subject, "s-two");
    assert_eq!(rows[0].state, IntentState::Done);
    assert_eq!(rows[1].state, IntentState::Done);
    absent(&scratch.file())
}
