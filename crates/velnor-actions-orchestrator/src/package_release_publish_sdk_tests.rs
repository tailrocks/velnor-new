use super::*;
use serde_json::{Value, json};

const TAP: &str = "jackin-project/homebrew-tap";
const HEAD_REPOSITORY: &str = TAP;
const SOURCE_SHA: &str = "a12e73afd75b944a872e8295d61a15f93f5789a2";
const BASE_SHA: &str = "c12e73afd75b944a872e8295d61a15f93f5789a2";
const PR_HEAD_SHA: &str = "d12e73afd75b944a872e8295d61a15f93f5789a2";
const PR_NUMBER: u64 = 1180;
const PR_BRANCH: &str = "preview-homebrew-adoption";

fn response() -> Value {
    json!({
        "id": 901,
        "run_attempt": 2,
        "head_sha": PR_HEAD_SHA,
        "head_branch": PR_BRANCH,
        "event": "pull_request",
        "status": "in_progress",
        "repository": { "id": 101, "full_name": TAP },
        "head_repository": { "id": 101, "full_name": HEAD_REPOSITORY },
        "referenced_workflows": [{
            "path": format!("tailrocks/velnor-new/{SDK_ADOPTION_CALLEE_PATH}@{SOURCE_SHA}"),
            "sha": SOURCE_SHA
        }],
        "pull_requests": [{
            "number": PR_NUMBER,
            "head": {
                "sha": PR_HEAD_SHA,
                "ref": PR_BRANCH,
                "repo": {
                    "id": 101,
                    "name": "homebrew-tap",
                    "url": "https://api.github.com/repos/jackin-project/homebrew-tap"
                }
            },
            "base": {
                "sha": BASE_SHA,
                "ref": "main",
                "repo": {
                    "id": 101,
                    "name": "homebrew-tap",
                    "url": "https://api.github.com/repos/jackin-project/homebrew-tap"
                }
            }
        }]
    })
}

fn validate(value: &Value) -> Result<(), OrchestratorError> {
    let response: super::super::github::WorkflowRunWithReferences =
        serde_json::from_value(value.clone()).expect("workflow run response shape");
    let selection = PublicSdkCallerSelection {
        repository: TAP.to_owned(),
        repository_id: 101,
        head_repository: HEAD_REPOSITORY.to_owned(),
        head_repository_id: 101,
        pull_request_number: PR_NUMBER,
        pull_request_base_sha: BASE_SHA.to_owned(),
        pull_request_head_sha: PR_HEAD_SHA.to_owned(),
        run_id: 901,
        run_attempt: 2,
        sdk_source_sha: SOURCE_SHA.to_owned(),
    };
    validate_adoption_caller_run(&response, &selection)
}

#[test]
fn public_caller_run_requires_the_exact_pinned_reusable_workflow() {
    validate(&response()).expect("exact caller and SDK reference should pass");

    let mut wrong_repository = response();
    wrong_repository["repository"]["id"] = json!(102);
    assert!(validate(&wrong_repository).is_err());

    let mut wrong_head_repository = response();
    wrong_head_repository["head_repository"]["full_name"] = json!("attacker/fork");
    assert!(validate(&wrong_head_repository).is_err());

    let mut wrong_attempt = response();
    wrong_attempt["run_attempt"] = json!(1);
    assert!(validate(&wrong_attempt).is_err());

    let mut wrong_head = response();
    wrong_head["head_sha"] = json!(SOURCE_SHA);
    assert!(validate(&wrong_head).is_err());

    let mut wrong_pr_head = response();
    wrong_pr_head["pull_requests"][0]["head"]["sha"] = json!(SOURCE_SHA);
    assert!(validate(&wrong_pr_head).is_err());

    let mut wrong_pr_head_repository = response();
    wrong_pr_head_repository["pull_requests"][0]["head"]["repo"]["id"] = json!(102);
    assert!(validate(&wrong_pr_head_repository).is_err());

    let mut wrong_pr_base = response();
    wrong_pr_base["pull_requests"][0]["base"]["sha"] = json!(SOURCE_SHA);
    assert!(validate(&wrong_pr_base).is_err());

    let mut wrong_pr_number = response();
    wrong_pr_number["pull_requests"][0]["number"] = json!(1179);
    assert!(validate(&wrong_pr_number).is_err());

    let mut wrong_event = response();
    wrong_event["event"] = json!("workflow_dispatch");
    assert!(validate(&wrong_event).is_err());

    let mut wrong_path = response();
    wrong_path["referenced_workflows"][0]["path"] = json!(format!(
        "tailrocks/velnor-new/.github/workflows/other.yml@{SOURCE_SHA}"
    ));
    assert!(validate(&wrong_path).is_err());

    let mut wrong_source = response();
    wrong_source["referenced_workflows"][0]["path"] = json!(format!(
        "tailrocks/velnor-new/{SDK_ADOPTION_CALLEE_PATH}@{PR_HEAD_SHA}"
    ));
    wrong_source["referenced_workflows"][0]["sha"] = json!(PR_HEAD_SHA);
    assert!(validate(&wrong_source).is_err());

    let mut extra_reference = response();
    extra_reference["referenced_workflows"]
        .as_array_mut()
        .expect("workflow list")
        .push(json!({
            "path": "attacker/repo/.github/workflows/other.yml@main",
            "sha": SOURCE_SHA,
            "ref": "refs/heads/main"
        }));
    assert!(validate(&extra_reference).is_err());
}
