//! Async lifecycle completion reconciliation over the bounded host transport.

use super::*;
use velnor_runner_github::{
    ActionsJobReconciliationReason as Reason, ActionsJobReconciliationState as State,
    ObservedScaleSetJob, reconcile_observed_scale_set_job_async,
};

const OWNER: &str = "ChainArgos";
const REPOSITORY: &str = "java-monorepo";
const TOKEN: &str = "actions-read-canary";

fn observed() -> ObservedScaleSetJob<'static> {
    ObservedScaleSetJob {
        scale_set_job_id: Some("opaque:scale-set-job/119"),
        workflow_run_id: Some(88),
        runner_id: Some(31),
        runner_name: Some("velnor-job-31"),
    }
}

fn run(attempt: u32, source: &str) -> String {
    format!(
        r#"{{"id":88,"path":".github/workflows/ci.yml","run_attempt":{attempt},"status":"completed","conclusion":"success","event":"push","head_sha":"abc123","head_repository":{{"full_name":"{source}"}}}}"#
    )
}

fn attempt_jobs(items: &[&str]) -> String {
    attempt_jobs_with_total(items.len(), items)
}

fn attempt_jobs_with_total(total: usize, items: &[&str]) -> String {
    format!(
        r#"{{"total_count":{},"jobs":[{}]}}"#,
        total,
        items.join(",")
    )
}

fn job(id: i64, run_id: i64, status: &str, runner_id: i64, runner_name: &str) -> String {
    let conclusion = if status == "completed" {
        "\"success\""
    } else {
        "null"
    };
    format!(
        r#"{{"id":{id},"run_id":{run_id},"status":"{status}","conclusion":{conclusion},"runner_id":{runner_id},"runner_name":"{runner_name}","runner_group_id":1,"runner_group_name":"Default"}}"#
    )
}

fn transport(responses: &[(&str, &str)]) -> FakeTransport {
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

#[test]
fn async_completion_uses_observed_runner_and_binds_api_before_each_get() {
    let run = run(2, "ChainArgos/java-monorepo");
    let unrelated = job(101, 88, "completed", 32, "other-runner");
    let matching = job(119, 88, "completed", 31, "velnor-job-31");
    let first_attempt = attempt_jobs(&[&unrelated]);
    let second_attempt = attempt_jobs(&[&matching]);
    let mut transport = transport(&[
        ("200", &run),
        ("200", &first_attempt),
        ("200", &second_attempt),
    ]);

    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut transport,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("valid read-only sequence");

    assert_eq!(result.state, State::Completed);
    assert_eq!(
        result.scale_set_job_id.as_deref(),
        Some("opaque:scale-set-job/119")
    );
    assert_eq!(result.attempt, Some(2));
    let actions_job = result.job.expect("unique Actions REST job");
    assert_eq!((actions_job.id, actions_job.run_id), (119, 88));
    assert_eq!(actions_job.runner_id, Some(31));
    assert_eq!(actions_job.runner_name.as_deref(), Some("velnor-job-31"));

    let state = transport.0.lock().expect("test transport lock");
    assert_eq!(
        state.events,
        ["bind-api", "send", "bind-api", "send", "bind-api", "send"]
    );
    assert_eq!(state.requests.len(), 3);
    assert_eq!(
        state.requests[0].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88"
    );
    assert_eq!(
        state.requests[1].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/1/jobs"
    );
    assert_eq!(
        state.requests[2].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs"
    );
    for request in &state.requests {
        assert_eq!(request.method, velnor_runner_github::Method::Get);
        assert_eq!(request.body, Vec::<u8>::new());
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
fn missing_observed_identity_and_attempt_cap_do_not_read_job_pages() {
    let mut missing = transport(&[]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut missing,
        OWNER,
        REPOSITORY,
        ObservedScaleSetJob {
            runner_id: None,
            ..observed()
        },
        TOKEN,
    ))
    .expect("missing event identity is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::RunnerIdMissing));
    assert_eq!(
        missing.0.lock().expect("test transport lock").requests,
        Vec::new()
    );

    let run = run(9, "ChainArgos/java-monorepo");
    let mut capped = transport(&[("200", &run)]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut capped,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("attempt cap is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::AttemptLimitExceeded));
    let state = capped.0.lock().expect("test transport lock");
    assert_eq!(state.requests.len(), 1);
    assert_eq!(state.events, ["bind-api", "send"]);
}

#[test]
fn pending_or_missing_rest_evidence_never_becomes_terminal() {
    let workflow_run = run(1, "ChainArgos/java-monorepo");
    let queued = job(119, 88, "queued", 31, "velnor-job-31");
    let page = attempt_jobs(&[&queued]);
    let mut pending = transport(&[("200", &workflow_run), ("200", &page)]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut pending,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("queued is nonterminal");
    assert_eq!(result.state, State::Pending);
    assert_eq!(result.attempt, Some(1));

    let mut missing = transport(&[("404", "not found")]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut missing,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("404 is unresolved and not retried");
    assert_eq!(result.state, State::NotFound);
    assert_eq!(result.reason, Some(Reason::WorkflowRunNotFound));
    assert_eq!(
        missing
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        1
    );

    let forked_run = run(1, "attacker-fork/java-monorepo");
    let mut forked = transport(&[("200", &forked_run)]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut forked,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("source mismatch remains nonterminal");
    assert_eq!(result.state, State::Mismatch);
    assert_eq!(result.reason, Some(Reason::SourceRepositoryMismatch));
    assert_eq!(
        forked.0.lock().expect("test transport lock").requests.len(),
        1
    );

    let mut missing_attempt = transport(&[("200", &workflow_run), ("404", "not found")]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut missing_attempt,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("attempt 404 is unresolved and not retried");
    assert_eq!(result.state, State::NotFound);
    assert_eq!(result.reason, Some(Reason::AttemptNotFound));
    assert_eq!(
        missing_attempt
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        2
    );
}

#[test]
fn async_page_scan_requires_all_rows_and_rejects_duplicate_ids() {
    let run = run(1, "ChainArgos/java-monorepo");
    let unrelated = (1..=100)
        .map(|id| job(id, 88, "completed", 1000 + id, "other-runner"))
        .collect::<Vec<_>>();
    let unrelated_refs = unrelated.iter().map(String::as_str).collect::<Vec<_>>();
    let first_page = attempt_jobs_with_total(101, &unrelated_refs);
    let target = job(101, 88, "completed", 31, "velnor-job-31");
    let second_page = attempt_jobs_with_total(101, &[&target]);
    let mut complete = transport(&[("200", &run), ("200", &first_page), ("200", &second_page)]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut complete,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("complete pages prove exact runner row");
    assert_eq!(result.state, State::Completed);
    let state = complete.0.lock().expect("test transport lock");
    assert_eq!(state.requests.len(), 3);
    assert_eq!(
        state.requests[1].query.as_deref(),
        Some("per_page=100&page=1")
    );
    assert_eq!(
        state.requests[2].query.as_deref(),
        Some("per_page=100&page=2")
    );
    drop(state);

    let repeated = job(1, 88, "completed", 31, "velnor-job-31");
    let duplicate_page = attempt_jobs_with_total(101, &[&repeated]);
    let mut incomplete = transport(&[
        ("200", &run),
        ("200", &first_page),
        ("200", &duplicate_page),
    ]);
    let result = block_on_ready(reconcile_observed_scale_set_job_async(
        &mut incomplete,
        OWNER,
        REPOSITORY,
        observed(),
        TOKEN,
    ))
    .expect("duplicate row cannot be terminal evidence");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::DuplicateAttemptJobId));
    assert!(result.job.is_none());
}
