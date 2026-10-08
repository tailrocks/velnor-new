use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_cache::cache_p08::check_mbx_before_fetch;

use super::impl_cache_fixtures::LABEL;

#[test]
fn typed_mbx_owner_precedes_cargo_source_fetch_independent_of_labels() {
    let fetch = Step {
        name: "Presentation-only fetch label".to_owned(),
        id: None,
        role: Some(StepRole::CargoSourcesFetch),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mbx = Step {
        name: "Presentation-only MBX label".to_owned(),
        id: None,
        role: Some(StepRole::MbxCache),
        condition: None,
        kind: StepKind::Action {
            uses: format!("jdx/mr-boxington-action@{}", "d".repeat(40)),
            with: BTreeMap::from([
                ("github-cache-mode".to_owned(), "objects".to_owned()),
                ("version".to_owned(), "1.21.1".to_owned()),
                ("toolchain".to_owned(), "1.98.1".to_owned()),
                ("cache-generation".to_owned(), "generation".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    let job = Job {
        outputs: Vec::new(),
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![mbx.clone(), fetch.clone()],
    };
    assert!(check_mbx_before_fetch("demo", &job).is_ok());
    let reversed = Job {
        steps: vec![fetch.clone(), mbx],
        ..job.clone()
    };
    assert!(check_mbx_before_fetch("demo", &reversed).is_err());
}
