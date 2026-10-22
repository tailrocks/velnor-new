use super::verify_current_preview_ci;
use crate::package_release_publish::github::GitHubApi;
use serde_json::{Value, json};
use velnor_actions_contract::package_release::PackageReleaseSource;

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}

fn workflow() -> Value {
    json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"})
}

fn run(id: u64, attempt: u64, source: &str) -> Value {
    json!({
        "id":id,"run_attempt":attempt,"path":".github/workflows/ci.yml",
        "head_sha":source,"head_branch":"main","event":"push","name":"CI",
        "workflow_id":7,"run_number":id,"head_repository":{"id":9},
        "status":"completed","conclusion":"success","repository":{"id":9}
    })
}

fn jobs(id: u64, source: &str) -> Value {
    json!({"total_count":1,"jobs":[{
        "id":id,"run_id":id,"name":"Required","head_sha":source,
        "status":"completed","conclusion":"success"
    }]})
}

#[test]
fn retained_current_ci2_is_authenticated_against_source_a_not_authority_b_or_coordinator_c() {
    let source_a = "a".repeat(40);
    let authority_b = "b".repeat(40);
    let coordinator_c = "c".repeat(40);
    let current_ci2 = (23_u64, 2_u32);
    let source = PackageReleaseSource {
        commit_sha: source_a.clone(),
        source_ref: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        event: velnor_actions_contract::package_release::PackageReleaseEvent::WorkflowRun,
        ci_run_id: 22,
        ci_run_attempt: 1,
    };
    let responses = vec![
        (route("actions/workflows/ci.yml"), workflow()),
        (
            route("actions/runs/23/attempts/2"),
            run(current_ci2.0, u64::from(current_ci2.1), &source_a),
        ),
        (
            route("actions/runs/23/attempts/2/jobs?per_page=100&page=1"),
            jobs(current_ci2.0, &source_a),
        ),
        (
            route(&format!(
                "actions/workflows/7/runs?head_sha={source_a}&branch=main&event=push&per_page=100&page=1"
            )),
            json!({"workflow_runs":[run(current_ci2.0, u64::from(current_ci2.1), &source_a)]}),
        ),
        (
            route("actions/runs/23"),
            run(current_ci2.0, u64::from(current_ci2.1), &source_a),
        ),
    ];
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);

    verify_current_preview_ci(&api, 9, &source, current_ci2)
        .expect("the retained current CI run must be authenticated against source A");

    let observed = fixture.observed();
    assert!(observed.iter().any(|path| path.contains(&source_a)));
    assert!(!observed.iter().any(|path| path.contains(&authority_b)));
    assert!(!observed.iter().any(|path| path.contains(&coordinator_c)));
    fixture.assert_drained();
}
