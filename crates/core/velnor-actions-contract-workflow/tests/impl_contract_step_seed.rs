//! Typed authority for the host tool-seed consumer.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::step_identity::{
    is_tool_seed_step, validate_step_sequence,
};
use velnor_actions_contract_workflow::{Step, StepKind, StepRole};

fn seed_step() -> Step {
    Step {
        name: "Restore Velnor tool seed".to_owned(),
        id: None,
        role: Some(StepRole::ToolSeed),
        condition: None,
        kind: StepKind::Action {
            uses: velnor_actions_contract_workflow::workflow::step_identity::TOOL_SEED_USES
                .to_owned(),
            with: BTreeMap::from([(
                "cache_key".to_owned(),
                "mise-v1-x86_64-unknown-linux-gnu-2026.9.18-75c77caa4018556f".to_owned(),
            )]),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn tool_seed_authority_uses_typed_role_and_fixed_payload() {
    let mut step = seed_step();
    validate_step_sequence(std::slice::from_ref(&step), "tool-seed")
        .expect("valid local seed consumer");
    assert!(is_tool_seed_step(&step));

    step.name = "Presentation-only label".to_owned();
    assert!(is_tool_seed_step(&step));

    step.role = None;
    assert!(!is_tool_seed_step(&step));
    step.role = Some(StepRole::ToolSeed);

    if let StepKind::Action { uses, .. } = &mut step.kind {
        *uses = "actions/checkout@0123456789abcdef0123456789abcdef01234567".to_owned();
    }
    assert!(!is_tool_seed_step(&step));
    assert!(
        validate_step_sequence(std::slice::from_ref(&step), "tool-seed")
            .expect_err("role must match the fixed local action")
            .to_string()
            .contains("role_kind_mismatch")
    );
}

#[test]
fn tool_seed_authority_rejects_conditional_or_ambiguous_inputs() {
    let mut conditional = seed_step();
    conditional.condition = Some("success()".to_owned());
    assert!(!is_tool_seed_step(&conditional));
    assert!(
        validate_step_sequence(&[conditional], "tool-seed")
            .expect_err("the seed consumer is unconditional")
            .to_string()
            .contains("tool_seed_conditional")
    );

    let mut extra_input = seed_step();
    if let StepKind::Action { with, .. } = &mut extra_input.kind {
        with.insert("path".to_owned(), "/tmp/other".to_owned());
    }
    assert!(!is_tool_seed_step(&extra_input));
    assert!(validate_step_sequence(&[extra_input], "tool-seed").is_err());

    let mut empty_key = seed_step();
    if let StepKind::Action { with, .. } = &mut empty_key.kind {
        with.insert("cache_key".to_owned(), String::new());
    }
    assert!(!is_tool_seed_step(&empty_key));
    assert!(validate_step_sequence(&[empty_key], "tool-seed").is_err());
}
