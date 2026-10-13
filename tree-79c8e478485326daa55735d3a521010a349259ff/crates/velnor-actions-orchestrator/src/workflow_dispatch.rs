//! Typed workflow-dispatch inputs for the hosted qualification path.

use velnor_actions_contract::{DispatchInput, DispatchInputType, WorkflowDispatch};

/// Build the closed, Velnor-only dispatch input set.
pub(super) fn qualification_dispatch() -> WorkflowDispatch {
    WorkflowDispatch {
        inputs: vec![
            input("campaign", true, DispatchInputType::String),
            DispatchInput {
                name: "phase".to_owned(),
                required: true,
                input_type: DispatchInputType::Choice,
                choices: vec![
                    "cold".to_owned(),
                    "control".to_owned(),
                    "third".to_owned(),
                    "useful_delta".to_owned(),
                    "warm".to_owned(),
                ],
                default: None,
            },
            input("predecessor_run_attempt", false, DispatchInputType::String),
            input("predecessor_run_id", false, DispatchInputType::String),
        ],
    }
}

/// Build one input with no default or choices.
fn input(name: &str, required: bool, input_type: DispatchInputType) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        required,
        input_type,
        choices: Vec::new(),
        default: None,
    }
}
