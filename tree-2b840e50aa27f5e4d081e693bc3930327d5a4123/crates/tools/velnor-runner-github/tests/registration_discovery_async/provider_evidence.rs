//! Exact-attempt provider read tests over the bounded async Actions transport.

use super::*;
use velnor_runner_github::{
    ActionsWorkflowAttemptEvidenceGap as Gap, ActionsWorkflowAttemptProviderRead as Read,
    BearerRole, RequestPurpose, read_actions_workflow_attempt_provider_evidence_async,
};

mod check_run_url;

const OWNER: &str = "ChainArgos";
const REPOSITORY: &str = "java-monorepo";
const TOKEN: &str = "actions-read-canary";
const RUN_ID: i64 = 88;
const REPOSITORY_ID: i64 = 829_618_808;
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn scripted_transport(responses: &[(&str, &str)]) -> FakeTransport {
    let transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend(
            responses
                .iter()
                .map(|(status, body)| response(status.parse().expect("status code"), body)),
        );
    transport
}

fn run(attempt: u32, sha: &str) -> String {
    format!(
        r#"{{"id":{RUN_ID},"run_attempt":{attempt},"path":".github/workflows/ci.yml@refs/heads/main","status":"completed","conclusion":"success","event":"push","head_sha":"{sha}","head_branch":"main","repository":{{"id":{REPOSITORY_ID},"full_name":"ChainArgos/java-monorepo"}},"head_repository":{{"id":{REPOSITORY_ID},"full_name":"ChainArgos/java-monorepo"}}}}"#
    )
}

fn job(id: i64, run_id: i64, sha: &str) -> String {
    format!(
        r#"{{"id":{id},"run_id":{run_id},"name":"compile (JDK 21)","head_sha":"{sha}","status":"completed","conclusion":"success","runner_id":31,"runner_name":"runner-31","runner_group_id":1,"runner_group_name":"Restricted","workflow_name":"CI","head_branch":"main","labels":["self-hosted","linux"]}}"#
    )
}

fn queued_unassigned_job(run_id: i64, sha: &str, runner_id: i64) -> String {
    format!(
        r#"{{"id":119,"run_id":{run_id},"name":"compile (JDK 21)","head_sha":"{sha}","status":"queued","conclusion":null,"runner_id":{runner_id},"runner_name":"","runner_group_id":0,"runner_group_name":"","workflow_name":"CI","head_branch":"main","labels":[]}}"#
    )
}

fn hosted_job_with_zero_group_id(run_id: i64, sha: &str) -> String {
    format!(
        r#"{{"id":120,"run_id":{run_id},"name":"Rust / GitHub hosted","head_sha":"{sha}","status":"completed","conclusion":"success","runner_id":1000061145,"runner_name":"GitHub Actions 1000061145","runner_group_id":0,"runner_group_name":"GitHub Actions","workflow_name":"CI","head_branch":"main","labels":["ubuntu-26.04"]}}"#
    )
}

fn jobs(total_count: usize, items: &[String]) -> String {
    format!(
        r#"{{"total_count":{total_count},"jobs":[{}]}}"#,
        items.join(",")
    )
}

fn artifact(run_id: i64, repository_id: i64, sha: &str) -> String {
    format!(
        r#"{{"id":555,"name":"linux-jdk21","size_in_bytes":123,"expired":false,"digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","workflow_run":{{"id":{run_id},"repository_id":{repository_id},"head_repository_id":{repository_id},"head_branch":"main","head_sha":"{sha}"}}}}"#
    )
}

fn artifacts(total_count: usize, items: &[String]) -> String {
    format!(
        r#"{{"total_count":{total_count},"artifacts":[{}]}}"#,
        items.join(",")
    )
}

#[test]
fn reads_exact_attempt_complete_jobs_and_run_scoped_artifacts() {
    let first = (1..=100).map(|id| job(id, RUN_ID, SHA)).collect::<Vec<_>>();
    let second = vec![job(101, RUN_ID, SHA)];
    let first_page = jobs(101, &first);
    let second_page = jobs(101, &second);
    let artifact_page = artifacts(1, &[artifact(RUN_ID, REPOSITORY_ID, SHA)]);
    let run_body = run(2, SHA);
    let mut transport = scripted_transport(&[
        ("200", run_body.as_str()),
        ("200", first_page.as_str()),
        ("200", second_page.as_str()),
        ("200", artifact_page.as_str()),
    ]);

    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut transport,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("bounded read succeeds");
    assert!(matches!(result, Read::Complete(_)));
    let Read::Complete(evidence) = result else {
        return;
    };
    assert_eq!(evidence.repository_id, REPOSITORY_ID);
    assert_eq!(evidence.repository_full_name, "ChainArgos/java-monorepo");
    assert_eq!(evidence.workflow_run_id, RUN_ID);
    assert_eq!(evidence.attempt, 2);
    assert_eq!(evidence.head_sha, SHA);
    assert_eq!(evidence.jobs.len(), 101);
    assert_eq!(evidence.jobs[0].name, "compile (JDK 21)");
    assert_eq!(evidence.jobs[100].id, 101);
    assert_eq!(evidence.artifacts.len(), 1);
    assert_eq!(evidence.artifacts[0].id, 555);
    assert_eq!(evidence.artifacts[0].workflow_run_id, RUN_ID);
    assert_eq!(evidence.artifacts[0].head_sha, SHA);
    assert!(!format!("{evidence:?}").contains(TOKEN));

    let state = transport.0.lock().expect("test transport lock");
    assert_provider_read_routes_and_headers(&state.events, &state.requests);
}

fn assert_provider_read_routes_and_headers(
    events: &[&str],
    requests: &[velnor_runner_github::SessionRequest],
) {
    assert_eq!(
        events,
        [
            "bind-api", "send", "bind-api", "send", "bind-api", "send", "bind-api", "send"
        ]
    );
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2"
    );
    assert_eq!(
        requests[1].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs"
    );
    assert_eq!(requests[1].query.as_deref(), Some("per_page=100&page=1"));
    assert_eq!(requests[2].query.as_deref(), Some("per_page=100&page=2"));
    assert_eq!(
        requests[3].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88/artifacts"
    );
    assert_eq!(requests[3].query.as_deref(), Some("per_page=100&page=1"));
    for request in requests {
        assert_eq!(request.method, velnor_runner_github::Method::Get);
        assert_eq!(request.body, Vec::<u8>::new());
        assert_eq!(request.purpose, RequestPurpose::ActionsRead);
        assert_eq!(request.bearer_role, BearerRole::GithubRestCredential);
        assert!(!format!("{request:?}").contains(TOKEN));
        assert_eq!(
            request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.as_str()),
            Some("Bearer actions-read-canary")
        );
    }
}

#[test]
fn queued_unassigned_runner_sentinels_normalize_to_absent_identity() {
    let queued = queued_unassigned_job(RUN_ID, SHA, 0);
    let hosted = hosted_job_with_zero_group_id(RUN_ID, SHA);
    let queued_page = jobs(2, &[queued, hosted]);
    let run_body = run(2, SHA);
    let artifact_page = artifacts(0, &[]);
    let mut transport = scripted_transport(&[
        ("200", run_body.as_str()),
        ("200", queued_page.as_str()),
        ("200", artifact_page.as_str()),
    ]);

    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut transport,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("provider-shaped unassigned runner sentinels are valid");
    let Read::Complete(evidence) = result else {
        panic!("complete provider inventory expected");
    };
    let job = &evidence.jobs[0];
    assert_eq!(job.status, "queued");
    assert_eq!(job.runner_id, None);
    assert_eq!(job.runner_name, None);
    assert_eq!(job.runner_group_id, None);
    assert_eq!(job.runner_group_name, None);
    let hosted = &evidence.jobs[1];
    assert_eq!(hosted.runner_id, Some(1_000_061_145));
    assert_eq!(hosted.runner_group_id, None);
    assert_eq!(hosted.runner_group_name.as_deref(), Some("GitHub Actions"));

    let invalid_runner = queued_unassigned_job(RUN_ID, SHA, -1);
    let invalid_control_name = queued_unassigned_job(RUN_ID, SHA, 0)
        .replace("\"runner_name\":\"\"", "\"runner_name\":\"bad\\u0001\"");
    for invalid_job in [invalid_runner, invalid_control_name] {
        let invalid_page = jobs(1, &[invalid_job]);
        let mut invalid_transport =
            scripted_transport(&[("200", run_body.as_str()), ("200", invalid_page.as_str())]);
        assert!(
            block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
                &mut invalid_transport,
                OWNER,
                REPOSITORY,
                RUN_ID,
                2,
                SHA,
                TOKEN,
            ))
            .is_err()
        );
    }
}

#[test]
fn attempt_and_head_sha_mismatches_stop_before_list_reads() {
    for (body, expected_gap) in [
        (run(1, SHA), Gap::AttemptMismatch),
        (
            run(2, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            Gap::HeadShaMismatch,
        ),
    ] {
        let mut transport = scripted_transport(&[("200", body.as_str())]);
        let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
            &mut transport,
            OWNER,
            REPOSITORY,
            RUN_ID,
            2,
            SHA,
            TOKEN,
        ))
        .expect("mismatch is a typed non-positive outcome");
        assert_eq!(result, Read::Unavailable(expected_gap));
        assert_eq!(transport.0.lock().expect("test lock").requests.len(), 1);
    }
}

#[test]
fn incomplete_job_pages_and_artifact_identity_mismatches_never_return_partial_evidence() {
    let first = (1..=100).map(|id| job(id, RUN_ID, SHA)).collect::<Vec<_>>();
    let first_page = jobs(101, &first);
    let run_body = run(2, SHA);
    let mut missing_page = scripted_transport(&[
        ("200", run_body.as_str()),
        ("200", first_page.as_str()),
        ("404", "missing page"),
    ]);
    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut missing_page,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("missing exact-attempt page is non-positive");
    assert_eq!(result, Read::Unavailable(Gap::AttemptJobsNotFound));
    assert_eq!(missing_page.0.lock().expect("test lock").requests.len(), 3);

    let one_job = jobs(1, &[job(1, RUN_ID, SHA)]);
    let bad_artifact = artifacts(1, &[artifact(RUN_ID + 1, REPOSITORY_ID, SHA)]);
    let run_body = run(2, SHA);
    let mut mismatch = scripted_transport(&[
        ("200", run_body.as_str()),
        ("200", one_job.as_str()),
        ("200", bad_artifact.as_str()),
    ]);
    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut mismatch,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("cross-run artifact metadata is non-positive");
    assert_eq!(result, Read::Unavailable(Gap::ArtifactIdentityMismatch));
    assert_eq!(mismatch.0.lock().expect("test lock").requests.len(), 3);
}

#[test]
fn page_cap_and_duplicate_ids_fail_closed_without_artifact_reads() {
    let oversized = r#"{"total_count":401,"jobs":[]}"#;
    let run_body = run(2, SHA);
    let mut capped = scripted_transport(&[("200", run_body.as_str()), ("200", oversized)]);
    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut capped,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("page cap is a typed non-positive outcome");
    assert_eq!(result, Read::Unavailable(Gap::JobPageLimitExceeded));
    assert_eq!(capped.0.lock().expect("test lock").requests.len(), 2);

    let repeated = vec![job(1, RUN_ID, SHA), job(1, RUN_ID, SHA)];
    let repeated_page = jobs(2, &repeated);
    let run_body = run(2, SHA);
    let mut duplicate =
        scripted_transport(&[("200", run_body.as_str()), ("200", repeated_page.as_str())]);
    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut duplicate,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("duplicate provider id is a typed non-positive outcome");
    assert_eq!(result, Read::Unavailable(Gap::DuplicateJobId));
    assert_eq!(duplicate.0.lock().expect("test lock").requests.len(), 2);
}
