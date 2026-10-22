use super::*;
use serde_json::json;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, package_release::PackageReleaseChannel,
};
fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![],
        companions: vec![],
    }
}
#[test]
fn continuation_stale_candidate_stops_before_ci_release_or_tag_reads() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            "repos/example/app/git/ref/heads/main".to_owned(),
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":"b".repeat(40)}}),
        )],
    );
    assert_eq!(
        select_preview(&api, &plan(), &"a".repeat(40), 9, 17)
            .expect("admitted read-only API result"),
        None
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 1);
}
#[test]
fn continuation_with_failed_exact_source_ci_does_not_discover_publication() {
    let sha = "a".repeat(40);
    let mut responses = vec![
        (
            "repos/example/app/git/ref/heads/main".to_owned(),
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":sha}}),
        ),
        (
            "repos/example/app/actions/workflows/ci.yml".to_owned(),
            json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"}),
        ),
    ];
    responses.push((
        format!("repos/example/app/actions/workflows/7/runs?head_sha={sha}&per_page=100&page=1"),
        json!({"workflow_runs":[{
            "id":22,"run_attempt":3,"run_number":12,"workflow_id":7,
            "name":"CI","path":".github/workflows/ci.yml","head_sha":sha,
            "head_branch":"main","event":"push","status":"completed",
            "conclusion":"failure","repository":{"id":9},"head_repository":{"id":9}
        }]}),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert_eq!(
        select_preview(&api, &plan(), &sha, 9, 17).expect("admitted read-only API result"),
        None
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);
}
