//! Read-only reconciliation from observed runner events to exact REST jobs.

use std::collections::VecDeque;

use velnor_runner_github::{
    ActionsJobReconciliationReason as Reason, ActionsJobReconciliationState as State, Exchange,
    Method, ObservedScaleSetJob, SessionRequest, Transport, TransportFail,
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

fn attempt_jobs(items: &[String]) -> String {
    attempt_jobs_with_total(items.len(), items)
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
fn completion_uses_observed_runner_and_run_not_opaque_job_id() {
    let mut script = Script::new(vec![
        reply(200, &run(2, Some(REPOSITORY))),
        reply(
            200,
            &attempt_jobs(&[job(101, 88, "completed", Some(32), Some("other-runner"))]),
        ),
        reply(
            200,
            &attempt_jobs(&[job(119, 88, "completed", Some(31), Some("velnor-job-31"))]),
        ),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut script,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque:job/119")),
        TOKEN,
    )
    .expect("valid read-only response sequence");

    assert_eq!(result.state, State::Completed);
    assert_eq!(result.reason, None);
    assert_eq!(result.scale_set_job_id.as_deref(), Some("opaque:job/119"));
    assert_eq!(result.observed_workflow_run_id, Some(88));
    assert_eq!(result.observed_runner_id, Some(31));
    assert_eq!(
        result.observed_runner_name.as_deref(),
        Some("velnor-job-31")
    );
    assert_eq!(result.attempt, Some(2));
    let job = result.job.expect("REST job row");
    assert_eq!((job.id, job.run_id), (119, 88));
    assert_eq!(job.status, "completed");
    assert_eq!(job.conclusion.as_deref(), Some("success"));
    assert_eq!(job.runner_id, Some(31));
    assert_eq!(job.runner_name.as_deref(), Some("velnor-job-31"));
    assert_eq!(job.runner_group_id, Some(3));
    let run = result.workflow_run.expect("workflow source evidence");
    assert_eq!(run.head_sha, "abc123");
    assert_eq!(run.head_repository_full_name.as_deref(), Some(REPOSITORY));

    let paths = script
        .seen
        .iter()
        .map(|request| request.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "repos/ChainArgos/java-monorepo/actions/runs/88",
            "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/1/jobs",
            "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs",
        ]
    );
    assert_eq!(script.seen[1].query.as_deref(), Some("per_page=100&page=1"));
    assert_eq!(script.seen[2].query.as_deref(), Some("per_page=100&page=1"));
    for request in &script.seen {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.body, Vec::<u8>::new());
        assert_eq!(
            header(request, "Authorization"),
            Some("Bearer actions-read-canary")
        );
        assert!(!format!("{request:?}").contains(TOKEN));
        assert!(!request.path.contains("/actions/jobs/"));
    }
}

#[test]
fn missing_observed_run_or_runner_identity_does_not_query_github() {
    let cases = [
        (
            ObservedScaleSetJob {
                workflow_run_id: None,
                ..observed(Some("opaque-job"))
            },
            Reason::WorkflowRunIdMissing,
        ),
        (
            ObservedScaleSetJob {
                runner_id: None,
                ..observed(Some("opaque-job"))
            },
            Reason::RunnerIdMissing,
        ),
        (
            ObservedScaleSetJob {
                runner_name: None,
                ..observed(Some("opaque-job"))
            },
            Reason::RunnerNameMissing,
        ),
    ];
    for (identity, reason) in cases {
        let mut script = Script::new(vec![]);
        let result = reconcile_observed_scale_set_job(
            &mut script,
            "ChainArgos",
            "java-monorepo",
            identity,
            TOKEN,
        )
        .expect("missing event identity is represented");
        assert_eq!(result.state, State::Unknown);
        assert_eq!(result.reason, Some(reason));
        assert!(result.job.is_none());
        assert_eq!(script.seen.len(), 0);
    }
}

#[test]
fn workflow_404_is_not_terminal_and_is_not_retried() {
    let mut script = Script::new(vec![reply(404, "not found")]);
    let result = reconcile_observed_scale_set_job(
        &mut script,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque-job")),
        TOKEN,
    )
    .expect("404 remains unresolved");
    assert_eq!(result.state, State::NotFound);
    assert_eq!(result.reason, Some(Reason::WorkflowRunNotFound));
    assert!(result.job.is_none());
    assert_eq!(script.seen.len(), 1);
}

#[test]
fn source_and_attempt_identity_must_match() {
    let mut wrong_source = Script::new(vec![reply(200, &run(1, Some("fork/java-monorepo")))]);
    let result = reconcile_observed_scale_set_job(
        &mut wrong_source,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque-job")),
        TOKEN,
    )
    .expect("source mismatch is represented");
    assert_eq!(result.state, State::Mismatch);
    assert_eq!(result.reason, Some(Reason::SourceRepositoryMismatch));
    assert_eq!(wrong_source.seen.len(), 1);

    let mut source_missing = Script::new(vec![reply(200, &run(1, None))]);
    let result = reconcile_observed_scale_set_job(
        &mut source_missing,
        "ChainArgos",
        "java-monorepo",
        observed(Some("opaque-job")),
        TOKEN,
    )
    .expect("missing source is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::SourceRepositoryMissing));
    assert_eq!(source_missing.seen.len(), 1);
}

#[test]
fn queued_and_in_progress_rows_remain_pending() {
    for status in ["queued", "in_progress"] {
        let mut script = Script::new(vec![
            reply(200, &run(1, Some(REPOSITORY))),
            reply(
                200,
                &attempt_jobs(&[job(119, 88, status, Some(31), Some("velnor-job-31"))]),
            ),
        ]);
        let result = reconcile_observed_scale_set_job(
            &mut script,
            "ChainArgos",
            "java-monorepo",
            observed(None),
            TOKEN,
        )
        .expect("valid pending state");
        assert_eq!(result.state, State::Pending);
        assert_eq!(result.attempt, Some(1));
        assert_eq!(
            result.job.as_ref().map(|job| job.status.as_str()),
            Some(status)
        );
    }
}

#[test]
fn partial_or_nonunique_runner_matches_stay_unresolved() {
    let mut partial = Script::new(vec![
        reply(200, &run(1, Some(REPOSITORY))),
        reply(
            200,
            &attempt_jobs(&[job(119, 88, "completed", Some(31), Some("renamed-runner"))]),
        ),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut partial,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("partial identity is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::RunnerIdentityMismatch));

    let mut multiple = Script::new(vec![
        reply(200, &run(2, Some(REPOSITORY))),
        reply(
            200,
            &attempt_jobs(&[job(119, 88, "completed", Some(31), Some("velnor-job-31"))]),
        ),
        reply(
            200,
            &attempt_jobs(&[job(120, 88, "completed", Some(31), Some("velnor-job-31"))]),
        ),
    ]);
    let result = reconcile_observed_scale_set_job(
        &mut multiple,
        "ChainArgos",
        "java-monorepo",
        observed(None),
        TOKEN,
    )
    .expect("ambiguous identity is unresolved");
    assert_eq!(result.state, State::Unknown);
    assert_eq!(result.reason, Some(Reason::MultipleRunnerJobMatches));
    assert!(result.job.is_none());
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}
