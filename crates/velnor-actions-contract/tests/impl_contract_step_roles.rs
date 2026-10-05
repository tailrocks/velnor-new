//! Typed step authority stays independent of presentation names.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};
use velnor_actions_contract::workflow::step_identity::validate_step_sequence;

fn plan_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: Some(StepId::Plan),
        role: Some(StepRole::PlanProducer),
        condition: None,
        kind: StepKind::Internal {
            operation: "plan-v1".to_owned(),
        },
    }
}

#[test]
fn display_name_does_not_change_typed_output_identity() {
    let mut step = plan_step("Plan");
    validate_step_sequence(std::slice::from_ref(&step), "plan").expect("valid plan owner");

    step.name = "A presentation-only label".to_owned();
    validate_step_sequence(std::slice::from_ref(&step), "plan").expect("rename preserves role");
    assert_eq!(step.id, Some(StepId::Plan));
    assert_eq!(step.role, Some(StepRole::PlanProducer));
}

#[test]
fn final_serialized_scope_rejects_duplicate_typed_ids() {
    let steps = [plan_step("Plan"), plan_step("Renamed Plan")];
    let error = validate_step_sequence(&steps, "plan").expect_err("duplicate output id");
    assert!(error.to_string().contains("duplicate_step_id:plan:plan"));
}

#[test]
fn role_kind_mismatch_fails_even_when_display_name_matches() {
    let step = Step {
        name: "Plan".to_owned(),
        id: None,
        role: Some(StepRole::PlanProducer),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let error = validate_step_sequence(&[step], "plan").expect_err("wrong semantic payload");
    assert!(error.to_string().contains("role_kind_mismatch"));
}
