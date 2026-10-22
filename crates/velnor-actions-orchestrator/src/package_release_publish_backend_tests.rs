use super::{verify_current_preview_ci, verify_preview_source_ci};
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
fn retained_recovery_authenticates_original_ci1_and_latest_ci2_against_a_not_b_or_c() {
    let source_a = "a".repeat(40);
    let authority_b = "b".repeat(40);
    let coordinator_c = "c".repeat(40);
    let original_ci1 = (22_u64, 1_u32);
    let latest_ci2 = (23_u64, 2_u32);
    let source = PackageReleaseSource {
        commit_sha: source_a.clone(),
        source_ref: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        event: velnor_actions_contract::package_release::PackageReleaseEvent::WorkflowRun,
        ci_run_id: original_ci1.0,
        ci_run_attempt: original_ci1.1,
    };
    let responses = vec![
        (route("actions/workflows/ci.yml"), workflow()),
        (
            route("actions/runs/22/attempts/1"),
            run(original_ci1.0, u64::from(original_ci1.1), &source_a),
        ),
        (
            route("actions/runs/22/attempts/1/jobs?per_page=100&page=1"),
            jobs(original_ci1.0, &source_a),
        ),
        (
            route(&format!(
                "actions/workflows/7/runs?head_sha={source_a}&per_page=100&page=1"
            )),
            json!({"workflow_runs":[run(latest_ci2.0, u64::from(latest_ci2.1), &source_a)]}),
        ),
        (
            route("actions/runs/23/attempts/2"),
            run(latest_ci2.0, u64::from(latest_ci2.1), &source_a),
        ),
        (
            route("actions/runs/23/attempts/2/jobs?per_page=100&page=1"),
            jobs(latest_ci2.0, &source_a),
        ),
        (
            route(&format!(
                "actions/workflows/7/runs?head_sha={source_a}&per_page=100&page=1"
            )),
            json!({"workflow_runs":[run(latest_ci2.0, u64::from(latest_ci2.1), &source_a)]}),
        ),
        (route("actions/runs/23"), run(latest_ci2.0, 2, &source_a)),
    ];
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);

    verify_preview_source_ci(&api, 9, &source)
        .expect("original and latest CI attempts are both authenticated for source A");

    let observed = fixture.observed();
    assert!(
        observed
            .iter()
            .any(|path| path.contains(&format!("head_sha={source_a}")))
    );
    assert!(!observed.iter().any(|path| path.contains(&authority_b)));
    assert!(!observed.iter().any(|path| path.contains(&coordinator_c)));
    assert_eq!((source.ci_run_id, source.ci_run_attempt), original_ci1);
    assert!(
        observed
            .iter()
            .any(|path| path.contains("actions/runs/23/attempts/2"))
    );
    fixture.assert_drained();
}

#[test]
fn retained_current_ci2_admission_uses_source_a_when_authority_b_and_coordinator_c_differ() {
    let source_a = "a".repeat(40);
    let authority_b = "b".repeat(40);
    let coordinator_c = "c".repeat(40);
    let latest_ci2 = (23_u64, 2_u32);
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
            run(latest_ci2.0, u64::from(latest_ci2.1), &source_a),
        ),
        (
            route("actions/runs/23/attempts/2/jobs?per_page=100&page=1"),
            jobs(latest_ci2.0, &source_a),
        ),
        (
            route(&format!(
                "actions/workflows/7/runs?head_sha={source_a}&per_page=100&page=1"
            )),
            json!({"workflow_runs":[run(latest_ci2.0, u64::from(latest_ci2.1), &source_a)]}),
        ),
        (
            route("actions/runs/23"),
            run(latest_ci2.0, u64::from(latest_ci2.1), &source_a),
        ),
    ];
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);

    verify_current_preview_ci(&api, 9, &source, latest_ci2)
        .expect("recovery's current CI2 must be authenticated against source A");

    let observed = fixture.observed();
    assert!(observed.iter().any(|path| path.contains(&source_a)));
    assert!(!observed.iter().any(|path| path.contains(&authority_b)));
    assert!(!observed.iter().any(|path| path.contains(&coordinator_c)));
    fixture.assert_drained();
}
