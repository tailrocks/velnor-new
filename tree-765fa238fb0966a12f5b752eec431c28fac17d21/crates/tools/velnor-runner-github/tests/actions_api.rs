//! Read-only workflow/job lookups used for trust and lifecycle reconciliation.

use std::collections::VecDeque;

use velnor_runner_github::{
    ActionsJob, ActionsWorkflowRun, Exchange, Method, SessionError, SessionRequest, Transport,
    TransportFail, WireError, get_actions_job, get_actions_workflow_run,
};

const TOKEN: &str = "actions-read-canary";

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn replies(bodies: &[&str]) -> Self {
        Self {
            replies: bodies
                .iter()
                .map(|body| {
                    Ok(Exchange {
                        status: 200,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

#[test]
fn actions_job_and_run_are_read_only_and_keep_distinct_identities() -> Result<(), &'static str> {
    let mut script = Script::replies(&[
        r#"{"id":119,"run_id":88,"status":"completed","conclusion":"success","runner_id":31,"runner_name":"velnor-job-31","runner_group_id":3,"runner_group_name":"Default"}"#,
        r#"{"id":88,"run_attempt":2,"status":"completed","conclusion":"success","event":"push","head_sha":"abc123","head_repository":{"full_name":"ChainArgos/java-monorepo"}}"#,
    ]);

    let job = get_actions_job(&mut script, "ChainArgos", "java-monorepo", "119", TOKEN)
        .map_err(|_| "job")?;
    let run = get_actions_workflow_run(&mut script, "ChainArgos", "java-monorepo", 88, TOKEN)
        .map_err(|_| "run")?;

    assert_eq!(
        job,
        Some(ActionsJob {
            id: 119,
            run_id: 88,
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
            runner_id: Some(31),
            runner_name: Some("velnor-job-31".to_owned()),
            runner_group_id: Some(3),
            runner_group_name: Some("Default".to_owned()),
        })
    );
    assert_eq!(
        run,
        ActionsWorkflowRun {
            id: 88,
            run_attempt: 2,
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
            event: "push".to_owned(),
            head_sha: "abc123".to_owned(),
            head_repository_full_name: Some("ChainArgos/java-monorepo".to_owned()),
        }
    );
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(
        script.seen[0].path,
        "repos/ChainArgos/java-monorepo/actions/jobs/119"
    );
    assert_eq!(
        script.seen[1].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88"
    );
    for request in &script.seen {
        assert_eq!(request.query, None);
        assert_eq!(
            header(request, "Accept"),
            Some("application/vnd.github+json")
        );
        assert_eq!(header(request, "X-GitHub-Api-Version"), Some("2026-03-10"));
        assert_eq!(
            header(request, "Authorization"),
            Some("Bearer actions-read-canary")
        );
        assert_eq!(request.body, Vec::<u8>::new());
        assert!(!format!("{request:?}").contains(TOKEN));
    }
    Ok(())
}

#[test]
fn opaque_job_id_is_preserved_by_caller_and_never_used_as_a_path() -> Result<(), &'static str> {
    let opaque_id = "gha:job/119?attempt=2";
    let mut script = Script::replies(&[]);
    let lookup = get_actions_job(&mut script, "ChainArgos", "java-monorepo", opaque_id, TOKEN)
        .map_err(|_| "opaque id")?;

    assert_eq!(opaque_id, "gha:job/119?attempt=2");
    assert_eq!(lookup, None);
    assert_eq!(script.seen, Vec::<SessionRequest>::new());
    Ok(())
}

#[test]
fn invalid_repository_path_and_empty_token_fail_before_transport() {
    let mut script = Script::replies(&[]);
    assert_eq!(
        get_actions_workflow_run(&mut script, "../org", "repo", 88, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        get_actions_workflow_run(&mut script, "org", "repo", 88, ""),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        get_actions_workflow_run(&mut script, "org", "repo", 0, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 0);
}

#[test]
fn response_identity_must_match_requested_job_and_run() {
    let mut wrong_job =
        Script::replies(&[r#"{"id":120,"run_id":88,"status":"completed","conclusion":"success"}"#]);
    assert_eq!(
        get_actions_job(&mut wrong_job, "org", "repo", "119", TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );

    let mut wrong_run = Script::replies(&[
        r#"{"id":88,"run_attempt":1,"status":"completed","event":"push","head_sha":"abc123","head_repository":null}"#,
    ]);
    assert_eq!(
        get_actions_workflow_run(&mut wrong_run, "org", "repo", 89, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}
