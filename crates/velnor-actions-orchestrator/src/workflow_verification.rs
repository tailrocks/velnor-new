//! Main CI cadence and manual-dispatch trigger construction.

use velnor_actions_contract::workflow::ir::{DispatchInput, DispatchInputType, WorkflowDispatch};
use velnor_actions_contract::{ScheduleTrigger, VerificationConfig};

/// Build optional main-workflow dispatch inputs in contract order.
pub(super) fn workflow_dispatch_trigger(config: &VerificationConfig) -> Option<WorkflowDispatch> {
    if !config.workflow_dispatch {
        return None;
    }
    let mut inputs = vec![
        DispatchInput {
            input_type: DispatchInputType::String,
            description: None,
            options: Vec::new(),
            name: "base_sha".to_owned(),
            required: false,
            default: None,
        },
        DispatchInput {
            input_type: DispatchInputType::String,
            description: None,
            options: Vec::new(),
            name: "scope".to_owned(),
            required: false,
            default: Some("full".to_owned()),
        },
    ];
    if config.alert {
        inputs.push(DispatchInput {
            name: "simulate_failure".to_owned(),
            input_type: DispatchInputType::Boolean,
            description: None,
            options: Vec::new(),
            required: false,
            default: Some("false".to_owned()),
        });
    }
    Some(WorkflowDispatch { inputs })
}

/// Build the configured main-workflow schedule.
pub(super) fn schedule_trigger(config: &VerificationConfig) -> Option<ScheduleTrigger> {
    config.schedule.as_ref().map(|cron| ScheduleTrigger {
        cron: vec![cron.clone()],
    })
}
