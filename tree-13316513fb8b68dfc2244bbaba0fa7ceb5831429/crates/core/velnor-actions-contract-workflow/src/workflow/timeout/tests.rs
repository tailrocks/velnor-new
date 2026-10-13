use super::JobTimeout;

#[test]
fn range_edges_hold_and_beyond_fails() {
    assert_eq!(JobTimeout::new(1).expect("fixture holds").minutes(), 1);
    assert_eq!(JobTimeout::new(360).expect("fixture holds").minutes(), 360);
    for bad in [0, 361, 1000, u16::MAX] {
        let err = JobTimeout::new(bad).expect_err("must reject");
        assert!(err.to_string().contains("bad_timeout"), "{err}");
    }
}

#[test]
fn every_default_is_a_valid_bound() {
    for default in JobTimeout::DEFAULTS {
        assert!(default.validate().is_ok(), "{default:?} must validate");
    }
    assert!(JobTimeout::CRATE.minutes() > JobTimeout::PLAN.minutes());
    assert!(JobTimeout::CRATE.minutes() > JobTimeout::VALIDATOR.minutes());
}

#[test]
fn deserialized_zero_still_fails_the_gate() {
    let timeout: JobTimeout = serde_json::from_str("0").expect("fixture holds");
    let err = timeout.validate().expect_err("serde bypass must not hold");
    assert!(err.to_string().contains("bad_timeout"), "{err}");
    let timeout: JobTimeout = serde_json::from_str("30").expect("fixture holds");
    assert!(timeout.validate().is_ok());
}

/// Minimal workflow JSON with a swappable job timeout.
fn workflow_json(timeout: &str) -> String {
    format!(
        r#"{{"name":"CI","triggers":{{"pull_request_types":[],"push_branches":["main"],"merge_group":false}},"permissions":{{"contents":"read","pull_requests":"none","id_token":"none","actions":"read"}},"concurrency":{{"group":"g","cancel_in_progress":"c"}},"jobs":{{"plan":{{"display_name":"Plan","runs_on":"ubuntu-26.04","timeout_minutes":{timeout},"steps":[{{"name":"s","kind":"shell","run":["true"]}}]}}}}}}"#
    )
}

#[test]
fn workflow_gate_rejects_deserialized_zero_and_absurd_timeouts() {
    use crate::workflow::ir::WorkflowIr;
    for bad in ["0", "361", "3600"] {
        let workflow: WorkflowIr =
            serde_json::from_str(&workflow_json(bad)).expect("fixture holds");
        let err = workflow.validate().expect_err("must reject");
        assert!(err.to_string().contains("bad_timeout"), "{err}");
    }
    let workflow: WorkflowIr = serde_json::from_str(&workflow_json("10")).expect("fixture holds");
    assert!(workflow.validate().is_ok());
}
