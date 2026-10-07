use super::common::{TRUSTED_WORKFLOW_REF, batch, offer};
use crate::policy::{PollWithTrust, WorkflowTrustField, parse_poll_with_trust};

#[test]
fn poll_preserves_each_event_and_its_exact_workflow_field_together() {
    let parsed = batch(
        7,
        &[
            offer(42, Some(TRUSTED_WORKFLOW_REF)),
            offer(43, None),
            offer(44, None).tap_invalid_workflow_ref(),
        ],
    );

    assert_eq!(parsed.message_id(), 7);
    assert_eq!(parsed.events().len(), 3);
    let first = parsed.event(0).expect("first event");
    assert_eq!(first.job().request_id, Some(42));
    assert_eq!(
        first.job_workflow_ref(),
        &WorkflowTrustField::Present(TRUSTED_WORKFLOW_REF.to_owned())
    );
    let second = parsed.event(1).expect("second event");
    assert_eq!(second.job().request_id, Some(43));
    assert_eq!(second.job_workflow_ref(), &WorkflowTrustField::Missing);
    let third = parsed.event(2).expect("third event");
    assert_eq!(third.job().request_id, Some(44));
    assert_eq!(third.job_workflow_ref(), &WorkflowTrustField::Invalid);
}

#[test]
fn empty_poll_is_not_a_batch_or_an_acknowledgement() {
    assert_eq!(
        parse_poll_with_trust(202, "").expect("empty response"),
        PollWithTrust::Empty
    );
}

trait InvalidWorkflowRef {
    fn tap_invalid_workflow_ref(self) -> Self;
}

impl InvalidWorkflowRef for serde_json::Value {
    fn tap_invalid_workflow_ref(mut self) -> Self {
        self["jobWorkflowRef"] = serde_json::json!(19);
        self
    }
}
