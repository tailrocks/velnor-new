//! Parallel step members carry no outputs, roles or planner operations.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::validate_step_sequence;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

fn shell(parallel: bool) -> Step {
    Step {
        parallel,
        name: "Clippy".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn plain_parallel_members_validate() {
    validate_step_sequence(&[shell(true), shell(true)], "rust-demo").expect("parallel members");
}

#[test]
fn parallel_members_reject_ids_roles_and_internal_operations() {
    let mut with_role = shell(true);
    with_role.role = Some(StepRole::MbxVersionCheck);
    with_role.id = Some(StepId::MbxReady);
    let internal = Step {
        kind: StepKind::Internal {
            operation: "plan-v1".to_owned(),
            env: BTreeMap::new(),
        },
        ..shell(true)
    };
    for step in [with_role, internal] {
        let error = validate_step_sequence(&[step], "rust-demo").expect_err("authority");
        assert!(
            error.to_string().contains("parallel_step_authority"),
            "{error}"
        );
    }
}

#[test]
fn sequential_steps_omit_the_parallel_field_in_serialized_ir() {
    let json = serde_json::to_string(&shell(false)).expect("serialize");
    assert!(!json.contains("parallel"), "{json}");
    let json = serde_json::to_string(&shell(true)).expect("serialize");
    assert!(json.contains("\"parallel\":true"), "{json}");
}
