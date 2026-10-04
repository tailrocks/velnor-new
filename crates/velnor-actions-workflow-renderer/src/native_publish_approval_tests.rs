use super::NativePublishApproval;
use velnor_actions_contract::WorkflowIr;

fn fixture() -> WorkflowIr {
    serde_json::from_str(include_str!(
        "../../velnor-actions-contract/tests/fixtures/native_publish.json"
    ))
    .expect("closed graph")
}
#[test]
fn approval_freezes_every_source_step_and_graph_binding() {
    let original = fixture();
    let approval = NativePublishApproval::compiled("attest", &original).expect("approval");
    assert!(approval.admits("attest", &original));
    assert!(!approval.admits("image-index", &original));
    let mut changed = original.clone();
    changed.jobs.get_mut("image-index").expect("producer").steps[0].name = "Foreign source".into();
    assert!(!approval.admits("attest", &changed));
    let mut changed = original.clone();
    changed.concurrency.cancel_in_progress = "true".into();
    assert!(!approval.admits("attest", &changed));
    let mut changed = original;
    changed.triggers.workflow_dispatch =
        Some(velnor_actions_contract::workflow::ir::WorkflowDispatch { inputs: Vec::new() });
    assert!(!approval.admits("attest", &changed));
}
#[test]
fn an_untyped_job_cannot_be_approved() {
    assert!(NativePublishApproval::compiled("image-index", &fixture()).is_err());
}

#[test]
fn generation_requires_one_exact_approval() {
    let workflow = fixture();
    let approval = NativePublishApproval::compiled("attest", &workflow).expect("approval");
    assert!(super::validate_approvals(&workflow, &[]).is_err());
    assert!(super::validate_approvals(&workflow, std::slice::from_ref(&approval)).is_ok());
    assert!(super::validate_approvals(&workflow, &[approval.clone(), approval]).is_err());
}
