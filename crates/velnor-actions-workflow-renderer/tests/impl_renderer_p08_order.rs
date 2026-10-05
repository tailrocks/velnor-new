//! P08 MBX/source-fetch ordering regression.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::cache_p08::check_mbx_before_fetch;

use super::*;

#[test]
fn mbx_action_precedes_fetch() {
    let fetch = Step {
        name: "Fetch Cargo sources".to_owned(),
        id: None,
        role: Some(StepRole::CargoSourcesFetch),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mbx = Step {
        name: "Restore MBX objects".to_owned(),
        id: None,
        role: Some(StepRole::MbxCache),
        condition: None,
        kind: StepKind::Action {
            uses: format!("jdx/mr-boxington-action@{}", "d".repeat(40)),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let good = Job {
        display_name: "Good".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![mbx.clone(), fetch.clone()],
    };
    assert!(check_mbx_before_fetch("demo", &good).is_ok());
    let bad = Job {
        steps: vec![fetch, mbx],
        ..good
    };
    assert!(check_mbx_before_fetch("demo", &bad).is_err());
}
