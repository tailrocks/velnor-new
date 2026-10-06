//! Typed manual validation and explicitly gated scheduled publication.
use velnor_actions_contract::workflow::{
    Trigger,
    dispatch::{DispatchInput, DispatchInputType, WorkflowDispatch},
    jobs::ScheduleTrigger,
};

pub(super) fn triggers(schedule: &str) -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_tags: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: Some(WorkflowDispatch {
            inputs: vec![
                input(
                    "channel",
                    "Package channel",
                    "stable",
                    &["stable", "preview"],
                ),
                input(
                    "commit",
                    "Target source commit (empty resolves it)",
                    "",
                    &[],
                ),
                input(
                    "mode",
                    "Delivery mode",
                    "validate",
                    &["validate", "publish"],
                ),
                input(
                    "version",
                    "Target version (empty discovers channel head)",
                    "",
                    &[],
                ),
            ],
        }),
        schedule: Some(ScheduleTrigger {
            cron: vec![schedule.to_owned()],
        }),
    }
}

fn input(name: &str, description: &str, default: &str, options: &[&str]) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        input_type: if options.is_empty() {
            DispatchInputType::String
        } else {
            DispatchInputType::Choice
        },
        required: false,
        description: Some(description.to_owned()),
        options: options.iter().map(|value| (*value).to_owned()).collect(),
        default: Some(default.to_owned()),
    }
}

pub(super) fn publication_condition(repository: &str, branch: &str) -> String {
    format!(
        "${{{{ github.repository == '{repository}' && github.ref == 'refs/heads/{branch}' && (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && inputs.mode == 'publish')) }}}}"
    )
}

pub(super) fn admission_condition(repository: &str, branch: &str) -> String {
    format!(
        "${{{{ github.repository == '{repository}' && github.ref == 'refs/heads/{branch}' && (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') }}}}"
    )
}
