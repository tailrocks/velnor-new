use crate::{RenderedFile, extract_workflow_authority};

fn workflow(events: &str) -> RenderedFile {
    RenderedFile {
        path: ".github/workflows/authority.yml".to_owned(),
        bytes: format!(
            "on: {events}\npermissions: {{}}\njobs:\n  check:\n    runs-on: ubuntu-26.04\n    steps: []\n"
        ),
    }
}

#[test]
fn pull_request_target_filters_apply_to_the_base_branch() {
    let file = workflow("{pull_request_target: {branches: [sea/integration]}}");
    let facts = extract_workflow_authority(&[file]).expect("Read the workflow facts.");
    let trigger = &facts[0].triggers[0];
    assert_eq!(trigger.event, "pull_request_target");
    assert_eq!(trigger.pull_request_base_branches, ["sea/integration"]);
    assert_eq!(trigger.branch_filters, Vec::<String>::new());
    assert_eq!(trigger.tag_filters, Vec::<String>::new());
}

#[test]
fn review_events_support_map_sequence_and_scalar_forms() {
    for events in [
        "issue_comment",
        "[issue_comment, pull_request_target]",
        "{issue_comment: {types: [created]}, pull_request_target: {types: [opened]}}",
    ] {
        let facts =
            extract_workflow_authority(&[workflow(events)]).expect("Read the workflow facts.");
        assert!(
            facts[0]
                .triggers
                .iter()
                .any(|event| event.event == "issue_comment")
        );
    }
}

#[test]
fn new_event_support_keeps_unknown_events_and_inexact_filters_closed() {
    for events in [
        "unknown_event",
        "[issue_comment, unknown_event]",
        "{unknown_event: {}}",
        "{pull_request_target: {branches: ['release/*']}}",
        "{pull_request_target: {branches-ignore: [main]}}",
    ] {
        assert!(extract_workflow_authority(&[workflow(events)]).is_err());
    }
}
