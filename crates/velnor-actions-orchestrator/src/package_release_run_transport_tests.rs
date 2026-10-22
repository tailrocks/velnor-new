use super::tests::{run_json, source};
use super::*;
use serde_json::{Value, json};

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}
fn metadata() -> Value {
    json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"})
}
fn required() -> Value {
    json!({"id":81,"run_id":22,"name":"Required","head_sha":source().sha,"status":"completed","conclusion":"success"})
}
fn required_with_id(id: u64) -> Value {
    let mut job = required();
    job["id"] = json!(id);
    job
}
fn list_route() -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={}&branch=main&event=push&per_page=100&page=1",
        source().sha
    ))
}
fn first_three(jobs: Value) -> Vec<(String, Value)> {
    vec![
        (route("actions/workflows/ci.yml"), metadata()),
        (route("actions/runs/22/attempts/3"), run_json()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            jobs,
        ),
    ]
}

#[test]
fn real_api_ci_admission_checks_full_dispatch_order_and_current_attempt() {
    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    responses.extend([
        (list_route(), json!({"workflow_runs":[run_json()]})),
        (route("actions/runs/22"), run_json()),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), true)
        .expect("fully authenticated current CI");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 5);
}

#[test]
fn real_api_stops_at_disabled_or_wrong_canonical_workflow_metadata() {
    for (field, value) in [
        ("state", json!("disabled_manually")),
        ("path", json!(".github/workflows/other.yml")),
        ("name", json!("Other CI")),
        ("id", json!(0)),
    ] {
        let mut response = metadata();
        response[field] = value;
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![(route("actions/workflows/ci.yml"), response)],
        );
        assert!(
            api.verify_ci_identity(22, 3, source(), true).is_err(),
            "accepted {field}"
        );
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 1);
    }
}

#[test]
fn real_api_rejects_stale_latest_or_new_attempt_before_publication() {
    for newer_at_list in [true, false] {
        let mut newer = run_json();
        newer["run_attempt"] = json!(4);
        let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
        responses.push((
            list_route(),
            json!({"workflow_runs":[if newer_at_list {newer.clone()} else {run_json()}]}),
        ));
        if !newer_at_list {
            responses.push((route("actions/runs/22"), newer));
        }
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), if newer_at_list { 4 } else { 5 });
    }
}

#[test]
fn real_api_rejects_duplicate_required_before_any_latest_lookup() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        first_three(json!({"total_count":2,"jobs":[required(),required()]})),
    );
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);
}

#[test]
fn real_api_reads_required_from_second_job_page_and_preserves_original_ci() {
    let fillers = (0..100).map(|n| json!({"id":n+1,"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})).collect::<Vec<_>>();
    let mut responses = first_three(json!({"total_count":101,"jobs":fillers}));
    responses.push((
        route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
        json!({"total_count":101,"jobs":[required_with_id(101)]}),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), false)
        .expect("original historical CI remains authenticated");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 4);
}

#[test]
fn unconfigured_mutation_is_rejected_without_any_process_or_network() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", vec![]);
    let error = api
        .post_fields(&route("releases"), &[])
        .expect_err("fixture must never fall through to gh");
    assert!(
        error.to_string().contains("unexpected_fixture_api_call"),
        "got {error}"
    );
    assert_eq!(
        fixture.observed(),
        vec![format!("POST {}", route("releases"))]
    );
    fixture.assert_drained();
}

#[test]
fn warm_completed_evidence_keeps_latest_list_and_current_run_fresh() {
    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    for _ in 0..2 {
        responses.extend([
            (list_route(), json!({"workflow_runs":[run_json()]})),
            (route("actions/runs/22"), run_json()),
        ]);
    }
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    for _ in 0..2 {
        api.verify_ci_identity(22, 3, source(), true)
            .expect("fresh latest CI");
    }
    fixture.assert_drained();
    assert_eq!(
        fixture.observed().len(),
        7,
        "immutable evidence read only once"
    );
}

#[test]
fn warm_completed_cache_never_accepts_new_attempt_pending_or_failed_current_ci() {
    for (field, value) in [
        ("run_attempt", json!(4)),
        ("status", json!("in_progress")),
        ("conclusion", json!("failure")),
        ("head_sha", json!("b".repeat(40))),
    ] {
        let mut changed = run_json();
        changed[field] = value;
        let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
        responses.extend([
            (list_route(), json!({"workflow_runs":[run_json()]})),
            (route("actions/runs/22"), changed),
        ]);
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        api.verify_ci_identity(22, 3, source(), false)
            .expect("original evidence");
        assert!(
            api.verify_ci_identity(22, 3, source(), true).is_err(),
            "accepted {field}"
        );
        fixture.assert_drained();
    }
}

#[test]
fn failed_required_evidence_is_not_cached_as_success() {
    let mut responses = first_three(json!({"total_count":0,"jobs":[]}));
    responses.extend([
        (route("actions/runs/22/attempts/3"), run_json()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            json!({"total_count":1,"jobs":[required()]}),
        ),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), false).is_err());
    api.verify_ci_identity(22, 3, source(), false)
        .expect("new actual success");
    api.verify_ci_identity(22, 3, source(), false)
        .expect("cached exact success");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 5);
}

#[test]
fn warm_completed_cache_still_rejects_a_newer_source_ci_run() {
    let mut newer = run_json();
    newer["id"] = json!(23);
    newer["run_number"] = json!(13);
    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    responses.push((list_route(), json!({"workflow_runs":[newer]})));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), false)
        .expect("original success");
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
}

fn release_receipt() -> PackageReleaseReceipt {
    serde_json::from_value(json!({
        "schema":"velnor.package-release.v1","channel":"preview",
        "repository":{"id":9,"slug":"example/app"},
        "source":{"commit_sha":source().sha,"ref":"refs/heads/main","default_branch":"main","event":"workflow_run","ci_run_id":22,"ci_run_attempt":3},
        "workflow":{"path":".github/workflows/binary-preview.yml","ref":"refs/heads/main","authority_sha":source().sha,"run_id":22,"run_attempt":3,"producer_run_id":22,"producer_run_attempt":3,"signer_job_id":"preview-signer","signer_workflow":".github/workflows/binary-preview.yml"},
        "version":{"base":"1.0.0","value":"1.0.0-preview.1+aaaaaaa","commit_count":1,"source_sha7":"aaaaaaa"},
        "tag":"preview","products":[],"companions":[],"assets":[],
        "evidence":{"profile":"signed-sbom-v1","sbom_format":"CycloneDX-JSON","signer_issuer":"https://token.actions.githubusercontent.com","signer_workflow_ref":"example/app/.github/workflows/binary-preview.yml@refs/heads/main","attestation_predicate":"https://slsa.dev/provenance/v1","attested_subject_names":[]}
    })).expect("typed run identity fixture")
}

fn producing_metadata() -> Value {
    json!({"id":8,"name":"Rust binary preview","path":".github/workflows/binary-preview.yml","state":"active"})
}

fn producing_run() -> Value {
    let mut run = run_json();
    run["workflow_id"] = json!(8);
    run["name"] = json!("Rust binary preview");
    run["path"] = json!(".github/workflows/binary-preview.yml");
    run["event"] = json!("workflow_run");
    run["status"] = json!("in_progress");
    run["conclusion"] = Value::Null;
    run
}

#[test]
fn mutation_guard_reads_current_producing_run_and_rejects_rerun_or_inactive_status() {
    for (pointer, value) in [
        ("/run_attempt", json!(4)),
        ("/status", json!("cancelled")),
        ("/status", json!("queued")),
        ("/status", json!("waiting")),
        ("/status", json!("completed")),
        ("/conclusion", json!("failure")),
        ("/repository/id", json!(10)),
        ("/workflow_id", json!(9)),
        ("/head_sha", json!("b".repeat(40))),
    ] {
        let mut run = producing_run();
        *run.pointer_mut(pointer).expect("fixture field") = value;
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![
                (
                    route("actions/workflows/binary-preview.yml"),
                    producing_metadata(),
                ),
                (route("actions/runs/22"), run),
            ],
        );
        assert!(
            api.verify_current_release_run(&release_receipt(), false)
                .is_err(),
            "{pointer}"
        );
        fixture.assert_drained();
    }
}

#[test]
fn historical_signed_attempt_remains_valid_while_new_mutation_rejects_rerun() {
    let mut original = producing_run();
    original["status"] = json!("completed");
    original["conclusion"] = json!("failure");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                route("actions/workflows/binary-preview.yml"),
                producing_metadata(),
            ),
            (route("actions/runs/22/attempts/3"), original.clone()),
            // Even though the aggregate run is now on attempt 4, retained
            // validation must keep authenticating the signed attempt 3.
            (route("actions/runs/22/attempts/3"), original),
            (route("actions/runs/22/attempts/3"), {
                let mut attempt = producing_run();
                attempt["status"] = json!("completed");
                attempt["conclusion"] = json!("failure");
                attempt
            }),
        ],
    );
    api.verify_release_run(&release_receipt(), true)
        .expect("historical signed attempt");
    api.verify_current_release_run(&release_receipt(), true)
        .expect("retained attempt remains valid after a newer rerun");
    api.verify_release_run(&release_receipt(), true)
        .expect("historical publication still valid");
    fixture.assert_drained();
}

#[test]
fn queued_workflow_admits_the_exact_running_publisher() {
    let mut run = producing_run();
    run["status"] = json!("queued");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                route("actions/workflows/binary-preview.yml"),
                producing_metadata(),
            ),
            (route("actions/runs/22"), run),
            (
                route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
                json!({"total_count":2,"jobs":[
            {"id":81,"run_id":22,"head_sha":source().sha,"name":"Publish authenticated channel release","status":"in_progress","conclusion":null,"started_at":"2026-10-20T20:54:22Z"},
            {"id":82,"run_id":22,"head_sha":source().sha,"name":"Other queued role","status":"queued","conclusion":null,"started_at":null}]}),
            ),
        ],
    );
    api.verify_current_release_run(&release_receipt(), false)
        .expect("executing publisher authority despite aggregate queue");
    fixture.assert_drained();
}

#[test]
fn terminal_publication_reads_keep_queued_original_distinct_from_active_tap_roles() {
    for role in [
        "Publish authenticated channel release",
        "Prepare a literal-only tap update without write credentials",
        "Independently validate and create a signed-off tap PR",
        "Merge only the validated and fully reviewed PR head",
    ] {
        let mut run = producing_run();
        run["status"] = json!("queued");
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![
                (
                    route("actions/workflows/binary-preview.yml"),
                    producing_metadata(),
                ),
                (route("actions/runs/22/attempts/3"), run.clone()),
                (route("actions/runs/22/attempts/3"), run.clone()),
                (route("actions/runs/22"), run),
                (
                    route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
                    json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"head_sha":source().sha,"name":role,"status":"in_progress","conclusion":null,"started_at":"2026-10-20T20:54:22Z"}]}),
                ),
            ],
        );
        api.verify_release_run(&release_receipt(), false)
            .expect("original exact-attempt read is not an aggregate execution claim");
        let workflow = api
            .admitted_workflow(
                ".github/workflows/binary-preview.yml",
                "Rust binary preview",
            )
            .expect("same workflow");
        api.verify_current_role(
            &CurrentRun {
                id: 22,
                attempt: 3,
                source: source(),
                workflow: &workflow,
                event: "workflow_run",
            },
            role,
        )
        .expect(
            "terminal verifier or direct Tap operation independently proves its executing role",
        );
        fixture.assert_drained();
    }
}

#[path = "package_release_direct_caller_tests.rs"]
mod direct_caller;
