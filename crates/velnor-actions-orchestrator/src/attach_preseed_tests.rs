//! Pre-seed anchor identity tests.

use std::collections::BTreeMap;

use super::preseed_anchor;
use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
use velnor_actions_workflow_renderer::steps::MBX_ACTION_NAME;

#[test]
fn preseed_anchor_uses_mbx_action_identity() {
    let steps = vec![
        Step {
            name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
            id: None,
            role: Some(StepRole::PreparePinnedTools),
            condition: None,
            kind: velnor_actions_contract::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        },
        Step {
            name: "Install MBX runtime".to_owned(),
            id: None,
            role: Some(StepRole::MbxCache),
            condition: None,
            kind: velnor_actions_contract::StepKind::Action {
                uses: format!("{MBX_ACTION_NAME}@{}", "d".repeat(40)),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        },
        Step {
            name: "Unrelated probe".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: velnor_actions_contract::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        },
    ];
    assert_eq!(preseed_anchor(&steps), 2);
}
