//! Bootstrap `version` dispatch binding and publisher-gate cases.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers, publish_gate_condition,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const REPO: &str = "acme/widgets";

/// Extract the `InvalidWorkflow` payload; `None` unless the exact rejection fired.
fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

fn dispatch(name: &str, required: bool, default: Option<&str>) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        description: format!("approved {name}"),
        required,
        default: default.map(str::to_owned),
    }
}

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-2026-09-30.1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates_io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
        version: None,
    }
}

fn triggers() -> ReleaseTriggers {
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![
            dispatch("plan", true, Some("plan-2026-09-30.1")),
            dispatch("source_sha", true, Some(SHA)),
        ],
    }
}

#[test]
fn dispatch_version_binds_approved_bootstrap_version() {
    let mut plan = bootstrap();
    plan.version = Some("1.2.3".to_owned());
    assert!(plan.validate().is_ok());
    assert_eq!(
        invalid(triggers().validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:version"
    );
    let mut bound = triggers();
    bound
        .dispatch_inputs
        .push(dispatch("version", true, Some("1.2.3")));
    assert!(bound.validate(&plan).is_ok());
    let mut rebound = triggers();
    rebound
        .dispatch_inputs
        .push(dispatch("version", true, Some("9.9.9")));
    assert_eq!(
        invalid(rebound.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:version"
    );
    let mut optional = triggers();
    optional
        .dispatch_inputs
        .push(dispatch("version", false, Some("1.2.3")));
    assert_eq!(
        invalid(optional.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:version"
    );
}

#[test]
fn dispatch_version_rejected_without_approved_version() {
    let plan = bootstrap();
    assert!(plan.version.is_none());
    let mut stray = triggers();
    stray
        .dispatch_inputs
        .push(dispatch("version", true, Some("1.2.3")));
    assert_eq!(
        invalid(stray.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:version"
    );
}

#[test]
fn publish_gate_appends_bootstrap_version() {
    let mut plan = bootstrap();
    plan.version = Some("1.2.3".to_owned());
    assert_eq!(
        publish_gate_condition(REPO, &plan),
        format!(
            "github.repository == '{REPO}' && github.event.inputs.plan == '{}' && github.event.inputs.source_sha == '{SHA}' && github.event.inputs.version == '1.2.3'",
            plan.plan_id,
        ),
        "versioned gate snapshot"
    );
    let mut malformed = bootstrap();
    malformed.version = Some("1.0".to_owned());
    assert!(
        invalid(malformed.validate())
            .expect("reject")
            .starts_with("bad_package_version:")
    );
}
