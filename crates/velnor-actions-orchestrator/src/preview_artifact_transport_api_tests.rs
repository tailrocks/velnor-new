use super::*;
use crate::package_release_publish::preview_artifacts::fixture::request;
use serde_json::{Value, json};

fn route(suffix: &str) -> String {
    format!("repos/example/app/{suffix}")
}
fn run(ci: bool) -> Value {
    let channel = PackageReleaseChannel::Preview;
    json!({"id":if ci {201} else {202}, "run_attempt":if ci {1} else {2},
        "run_number":5, "workflow_id":if ci {7} else {8},
        "name":if ci {"CI"} else {channel.workflow_name()},
        "path":if ci {".github/workflows/ci.yml"} else {channel.workflow_path()},
        "head_sha":"a".repeat(40), "head_branch":"main", "event":if ci {"push"} else {"workflow_run"},
        "status":if ci {"completed"} else {"in_progress"},
        "conclusion":if ci {json!("success")} else {Value::Null},
        "repository":{"id":101}, "head_repository":{"id":101}})
}
fn consumer_job() -> Value {
    json!({"id":404,"run_id":202,"name":"preview-assemble","head_sha":"a".repeat(40),
        "status":"in_progress","conclusion":null})
}
fn responses() -> Vec<(String, Value)> {
    let channel = PackageReleaseChannel::Preview;
    let filename = channel
        .workflow_path()
        .rsplit('/')
        .next()
        .expect("filename");
    vec![
        (
            "repos/example/app".into(),
            json!({"id":101,"full_name":"example/app"}),
        ),
        (
            route(&format!("actions/workflows/{filename}")),
            json!({"id":8,
            "name":channel.workflow_name(),"path":channel.workflow_path(),"state":"active"}),
        ),
        (route("actions/runs/202/attempts/2"), run(false)),
        (route("actions/runs/202"), run(false)),
        (
            route("actions/runs/202/attempts/2/jobs?per_page=100&page=1"),
            json!({"total_count":1,"jobs":[consumer_job()]}),
        ),
        (
            route("git/ref/heads/main"),
            json!({"ref":"refs/heads/main",
            "object":{"type":"commit","sha":"a".repeat(40)}}),
        ),
        (
            route("actions/workflows/ci.yml"),
            json!({"id":7,"name":"CI",
            "path":".github/workflows/ci.yml","state":"active"}),
        ),
        (route("actions/runs/201/attempts/1"), run(true)),
        (
            route("actions/runs/201/attempts/1/jobs?per_page=100&page=1"),
            json!({"jobs":[{
            "run_id":201,"name":"Required","head_sha":"a".repeat(40),
            "status":"completed","conclusion":"success"}]}),
        ),
        (
            route(&format!(
                "actions/workflows/7/runs?head_sha={}&branch=main&event=push&per_page=100&page=1",
                "a".repeat(40)
            )),
            json!({"workflow_runs":[run(true)]}),
        ),
        (route("actions/runs/201"), run(true)),
    ]
}

#[test]
fn read_capability_authenticates_current_and_exact_attempt_workflow_source_and_ci() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses());
    let read = ReleaseArtifactApi {
        api,
        repository: "example/app".into(),
    };
    read.authenticate(
        &request().plan,
        &request().context,
        ReleaseConsumer::Assembler,
    )
    .expect("complete remote authentication");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 11);
}

#[test]
fn stale_attempt_wrong_source_fork_workflow_or_repository_fail_before_artifact_io() {
    for (index, key, value) in [
        (0, "id", json!(102)),
        (0, "full_name", json!("other/app")),
        (1, "state", json!("disabled_manually")),
        (1, "path", json!("other.yml")),
        (1, "name", json!("Other")),
        (1, "id", json!(0)),
        (2, "run_attempt", json!(1)),
        (3, "run_attempt", json!(3)),
        (3, "id", json!(203)),
        (3, "workflow_id", json!(9)),
        (3, "event", json!("workflow_dispatch")),
        (3, "head_sha", json!("b".repeat(40))),
        (3, "repository", json!({"id":102})),
        (3, "head_repository", json!({"id":102})),
        (3, "status", json!("completed")),
        (3, "conclusion", json!("failure")),
        (3, "head_branch", json!("other")),
        (5, "object", json!({"type":"commit","sha":"b".repeat(40)})),
        (6, "name", json!("Other CI")),
        (7, "conclusion", json!("failure")),
        (10, "run_attempt", json!(2)),
    ] {
        let mut responses = responses();
        responses[index].1[key] = value;
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        let read = ReleaseArtifactApi {
            api,
            repository: "example/app".into(),
        };
        assert!(
            read.authenticate(
                &request().plan,
                &request().context,
                ReleaseConsumer::Assembler
            )
            .is_err(),
            "accepted field {index}/{key}"
        );
        assert!(
            fixture
                .observed()
                .iter()
                .all(|route| !route.contains("/artifacts"))
        );
    }
}

#[test]
fn page_reads_exact_run_page_and_requires_head_repository_identity_in_records() {
    let record = json!({"id":303,"name":"expected","expired":false,"size_in_bytes":16,
        "digest":format!("sha256:{}", "b".repeat(64)),"workflow_run":{"id":202,
        "repository_id":101,"head_repository_id":101,"head_sha":"a".repeat(40),"head_branch":"main"}});
    let path = route("actions/runs/202/artifacts?per_page=100&page=2");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            path,
            json!({"total_count":101,"artifacts":[record.clone()]}),
        )],
    );
    let read = ReleaseArtifactApi {
        api,
        repository: "example/app".into(),
    };
    let page = read.page(202, 2).expect("fixed read endpoint");
    assert_eq!(page.artifacts[0].id, 303);
    fixture.assert_drained();
    let mut malformed = record;
    malformed["workflow_run"]
        .as_object_mut()
        .expect("object")
        .remove("head_repository_id");
    assert!(
        serde_json::from_value::<ArtifactPage>(json!({"total_count":1,
        "artifacts":[malformed]}))
        .is_err()
    );
}

#[test]
fn queued_run_is_admitted_only_with_exact_unique_executing_assembler() {
    let mut valid = responses();
    valid[2].1["status"] = json!("queued");
    valid[3].1["status"] = json!("queued");
    let (api, fixture) = GitHubApi::with_fixture("example/app", valid);
    ReleaseArtifactApi {
        api,
        repository: "example/app".into(),
    }
    .authenticate(
        &request().plan,
        &request().context,
        ReleaseConsumer::Assembler,
    )
    .expect("queued run with executing native consumer");
    fixture.assert_drained();
    for (field, value) in [
        ("status", json!("queued")),
        ("name", json!("unrelated")),
        ("run_id", json!(203)),
        ("head_sha", json!("b".repeat(40))),
        ("conclusion", json!("failure")),
        ("id", json!(0)),
    ] {
        let mut queued = responses();
        queued[3].1["status"] = json!("queued");
        queued[4].1["jobs"][0][field] = value;
        let (api, _) = GitHubApi::with_fixture("example/app", queued);
        assert!(
            ReleaseArtifactApi {
                api,
                repository: "example/app".into()
            }
            .authenticate(
                &request().plan,
                &request().context,
                ReleaseConsumer::Assembler
            )
            .is_err(),
            "accepted consumer {field}"
        );
    }
    for jobs in [vec![], vec![consumer_job(), consumer_job()]] {
        let mut duplicate = responses();
        duplicate[4].1 = json!({"total_count":jobs.len(),"jobs":jobs});
        let (api, _) = GitHubApi::with_fixture("example/app", duplicate);
        assert!(
            ReleaseArtifactApi {
                api,
                repository: "example/app".into()
            }
            .authenticate(
                &request().plan,
                &request().context,
                ReleaseConsumer::Assembler
            )
            .is_err()
        );
    }
}

#[test]
fn assembler_job_inventory_requires_complete_stable_pages_and_unique_ids() {
    let fillers = (1..=100)
        .map(|id| {
            let mut job = consumer_job();
            job["id"] = json!(id);
            job["name"] = json!(format!("other-{id}"));
            job
        })
        .collect::<Vec<_>>();
    let pages = [
        json!({"total_count":101,"jobs":fillers}),
        json!({"total_count":101,"jobs":[consumer_job()]}),
    ];
    let queued = |pages: Vec<Value>| {
        pages
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                (
                    route(&format!(
                        "actions/runs/202/attempts/2/jobs?per_page=100&page={}",
                        index + 1
                    )),
                    page,
                )
            })
            .collect::<Vec<_>>()
    };
    let (api, fixture) = GitHubApi::with_fixture("example/app", queued(pages.to_vec()));
    ReleaseArtifactApi {
        api,
        repository: "example/app".into(),
    }
    .require_active_consumer(
        PackageReleaseChannel::Preview,
        &request().context,
        ReleaseConsumer::Assembler,
    )
    .expect("second-page exact consumer");
    fixture.assert_drained();
    let mut cases = Vec::new();
    let mut truncated = pages.to_vec();
    truncated[0]["jobs"].as_array_mut().expect("jobs").pop();
    cases.push(truncated);
    let mut changed = pages.to_vec();
    changed[1]["total_count"] = json!(102);
    cases.push(changed);
    let mut duplicate = pages.to_vec();
    duplicate[1]["jobs"][0]["id"] = json!(1);
    cases.push(duplicate);
    let mut empty = pages.to_vec();
    empty[1]["jobs"] = json!([]);
    cases.push(empty);
    let mut exceeded = pages.to_vec();
    exceeded[0]["total_count"] = json!(2001);
    cases.push(exceeded);
    for pages in cases {
        let (api, _) = GitHubApi::with_fixture("example/app", queued(pages));
        assert!(
            ReleaseArtifactApi {
                api,
                repository: "example/app".into()
            }
            .require_active_consumer(
                PackageReleaseChannel::Preview,
                &request().context,
                ReleaseConsumer::Assembler
            )
            .is_err()
        );
    }
}

#[test]
fn source_free_signer_requires_its_own_active_job_not_the_assembler() {
    let request = request();
    let (api, _) = GitHubApi::with_fixture("example/app", responses());
    assert!(
        ReleaseArtifactApi {
            api,
            repository: "example/app".into()
        }
        .authenticate(&request.plan, &request.context, ReleaseConsumer::Signer)
        .is_err()
    );
    let mut signer = responses();
    signer[4].1["jobs"][0]["name"] = serde_json::json!("preview-signer");
    let (api, fixture) = GitHubApi::with_fixture("example/app", signer);
    ReleaseArtifactApi {
        api,
        repository: "example/app".into(),
    }
    .authenticate(&request.plan, &request.context, ReleaseConsumer::Signer)
    .expect("exact native signer");
    fixture.assert_drained();
}
