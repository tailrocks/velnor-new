//! Read-only reconciliation from observed runner events to exact REST jobs.

use std::collections::VecDeque;

use velnor_runner_github::{
    ActionsJobReconciliationReason as Reason, ActionsJobReconciliationState as State, Exchange,
    ObservedScaleSetJob, SessionRequest, Transport, TransportFail,
    reconcile_observed_scale_set_job,
};

const TOKEN: &str = "actions-read-canary";
const REPOSITORY: &str = "ChainArgos/java-monorepo";

struct Reply {
    status: u16,
    body: String,
}

struct Script {
    replies: VecDeque<Reply>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn new(replies: Vec<Reply>) -> Self {
        Self {
            replies: replies.into(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        let reply = self.replies.pop_front().ok_or(TransportFail::Reset)?;
        Ok(Exchange {
            status: reply.status,
            body: reply.body.into_bytes(),
        })
    }
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.to_owned(),
    }
}

fn job(
    id: i64,
    run_id: i64,
    status: &str,
    runner_id: Option<i64>,
    runner_name: Option<&str>,
) -> String {
    let conclusion = if status == "completed" {
        "\"success\""
    } else {
        "null"
    };
    let runner_id = runner_id.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let runner_name = runner_name.map_or_else(|| "null".to_owned(), |value| format!("\"{value}\""));
    format!(
        r#"{{"id":{id},"run_id":{run_id},"status":"{status}","conclusion":{conclusion},"runner_id":{runner_id},"runner_name":{runner_name},"runner_group_id":3,"runner_group_name":"Default"}}"#
    )
}

fn run(attempt: i64, head_repository: Option<&str>) -> String {
    let repository = head_repository.map_or_else(
        || "null".to_owned(),
        |name| format!(r#"{{"full_name":"{name}"}}"#),
    );
    format!(
        r#"{{"id":88,"path":".github/workflows/ci.yml","run_attempt":{attempt},"status":"completed","conclusion":"success","event":"push","head_sha":"abc123","head_repository":{repository}}}"#
    )
}

fn attempt_jobs_with_total(total: usize, items: &[String]) -> String {
    format!(r#"{{"total_count":{total},"jobs":[{}]}}"#, items.join(","))
}

fn observed(job_id: Option<&str>) -> ObservedScaleSetJob<'_> {
    ObservedScaleSetJob {
        scale_set_job_id: job_id,
        workflow_run_id: Some(88),
        runner_id: Some(31),
        runner_name: Some("velnor-job-31"),
    }
}

#[test]
fn attempt_and_page_caps_fail_closed() {
    let mut attempts = Script::new(vec![reply(200, &run(9, Some(REPOSITORY)))]);
    let result = reconcile_observed_scale_set_job(
        &mut attempts,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("attempt limit is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::AttemptLimitExceeded));
    assert_eq!(attempts.seen.len(), 1);

    let mut pages = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(200, r#"{"total_count":401,"jobs":[]}"#),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut pages,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("page limit is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::PaginationLimitExceeded));
    assert_eq!(pages.seen.len(), 2);
}

#[test]
fn pagination_is_complete_before_job_is_accepted() {
    let first = (1000..1100)
        .map(|id| job(id, 88, "completed", Some(32), Some("other-runner")))
        .collect::<Vec<_>>();
    let second = vec![job(119, 88, "completed", Some(31), Some("velnor-job-31"))];
    let mut script = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(200, &attempt_jobs_with_total(101, &first)),
        reply(200, &attempt_jobs_with_total(101, &second)),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut script,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque-job")),
        TOKEN,
    )
    .expect("second page contains the exact runner identity");
    assert_eq!(result.state, State::Completed);
    assert_eq!(result.job.as_ref().map(|job| job.id), Some(119));
    assert_eq!(script.seen[2].query.as_deref(), Some("per_page=100&page=2"));
}

#[test]
fn duplicate_job_id_across_full_pages_cannot_hide_an_unseen_match() {
    let mut first = (1000..1100)
        .map(|id| job(id, 88, "completed", Some(32), Some("other-runner")))
        .collect::<Vec<_>>();
    first[0] = job(1000, 88, "completed", Some(31), Some("velnor-job-31"));
    // Page two repeats an unrelated row ID while the total still claims 101.
    // The omitted row could be a second matching job, so uniqueness is unknown.
    let second = vec![job(1001, 88, "completed", Some(32), Some("other-runner"))];
    let mut script = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(200, &attempt_jobs_with_total(101, &first)),
        reply(200, &attempt_jobs_with_total(101, &second)),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut script,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque-job")),
        TOKEN,
    )
    .expect("duplicate listing remains unresolved");

    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::DuplicateAttemptJobId));
    assert_eq!(result.job, None);
    assert_eq!(script.seen.len(), 3);
}

#[test]
fn incomplete_or_missing_attempt_pages_never_prove_completion() {
    let mut incomplete = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(
            200,
            &attempt_jobs_with_total(
                2,
                &[job(119, 88, "completed", Some(31), Some("velnor-job-31"))],
            ),
        ),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut incomplete,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("incomplete pagination is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::IncompleteAttemptPage));

    let mut missing_attempt = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(404, "not found"),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut missing_attempt,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("attempt 404 remains unresolved");
    assert_eq!(result.state, State::NotFound);
    assert_eq!(result.reason, Some(Reason::AttemptNotFound));
    assert_eq!(missing_attempt.seen.len(), 2);
}
