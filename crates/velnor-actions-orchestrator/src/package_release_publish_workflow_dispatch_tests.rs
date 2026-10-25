use super::{QualificationRunSelection, validate_run};
use crate::package_release_publish::github::WorkflowRunResponse;
use serde_json::json;

const REPOSITORY: &str = "owner/project";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RUN_ID: u64 = 42;
const ATTEMPT: u32 = 2;
const WORKFLOW_ID: u64 = 7;

fn selection() -> QualificationRunSelection {
    QualificationRunSelection::new(
        REPOSITORY.to_owned(),
        11,
        SHA.to_owned(),
        SHA.to_owned(),
        format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
        RUN_ID,
        ATTEMPT,
    )
}

fn run() -> WorkflowRunResponse {
    serde_json::from_value(run_value()).expect("workflow run fixture")
}

fn run_value() -> serde_json::Value {
    json!({
        "id": RUN_ID,
        "run_attempt": ATTEMPT,
        "workflow_id": WORKFLOW_ID,
        "path": ".github/workflows/ci.yml",
        "event": "workflow_dispatch",
        "status": "in_progress",
        "head_sha": SHA,
        "head_branch": "main",
        "repository": {"id": 11, "full_name": REPOSITORY},
        "head_repository": {"id": 11, "full_name": REPOSITORY}
    })
}

#[test]
fn qualification_run_requires_exact_active_ci_attempt_on_protected_main_identity() {
    let selected = selection();
    assert!(validate_run(&run(), WORKFLOW_ID, &selected).is_ok());

    let mutations: [fn(&mut serde_json::Value); 8] = [
        |value| value["event"] = json!("push"),
        |value| value["head_branch"] = json!("tag-name"),
        |value| value["head_sha"] = json!("b".repeat(40)),
        |value| value["path"] = json!(".github/workflows/other.yml"),
        |value| value["workflow_id"] = json!(99),
        |value| value["run_attempt"] = json!(1),
        |value| value["repository"]["id"] = json!(12),
        |value| value["head_repository"]["full_name"] = json!("fork/project"),
    ];
    for mutate in mutations {
        let mut value = run_value();
        mutate(&mut value);
        let changed: WorkflowRunResponse = serde_json::from_value(value).expect("changed run");
        assert!(validate_run(&changed, WORKFLOW_ID, &selected).is_err());
    }
}
