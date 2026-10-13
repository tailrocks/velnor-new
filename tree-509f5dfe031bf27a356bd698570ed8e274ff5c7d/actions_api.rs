//! Read-only workflow/job lookups used for trust and lifecycle reconciliation.

use std::collections::VecDeque;

use velnor_runner_github::{
    ActionsJob, ActionsRepository, ActionsWorkflowRun, Exchange, ForkPullRequestWorkflowSetting,
    Method, PrivateRepoForkWorkflowSettings, SessionError, SessionRequest, Transport,
    TransportFail, WireError, get_actions_job, get_actions_repository, get_actions_workflow_run,
    get_private_repo_fork_workflow_settings,
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
        r#"{"id":88,"path":".github/workflows/ci.yml","run_attempt":2,"status":"completed","conclusion":"success","event":"push","head_sha":"abc123","head_repository":{"full_name":"ChainArgos/java-monorepo"}}"#,
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
            path: ".github/workflows/ci.yml".to_owned(),
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
        assert_eq!(request.body, [] as [u8; 0]);
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
        r#"{"id":88,"path":".github/workflows/ci.yml","run_attempt":1,"status":"completed","event":"push","head_sha":"abc123","head_repository":null}"#,
    ]);
    assert_eq!(
        get_actions_workflow_run(&mut wrong_run, "org", "repo", 89, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
}

#[test]
fn repository_and_fork_policy_reads_are_explicit_read_only_inputs() -> Result<(), &'static str> {
    let mut script = Script::replies(&[
        r#"{"id":829618808,"full_name":"ChainArgos/java-monorepo","private":true,"visibility":"private","permissions":{"admin":true}}"#,
        r#"{"run_workflows_from_fork_pull_requests":false,"send_write_tokens_to_workflows":false,"send_secrets_and_variables":false,"require_approval_for_fork_pr_workflows":false}"#,
    ]);

    let repo = get_actions_repository(&mut script, "ChainArgos", "java-monorepo", TOKEN)
        .map_err(|_| "repo metadata")?;
    let fork_settings =
        get_private_repo_fork_workflow_settings(&mut script, "ChainArgos", "java-monorepo", TOKEN)
            .map_err(|_| "fork settings")?;

    assert_eq!(
        repo,
        ActionsRepository {
            id: 829_618_808,
            full_name: "ChainArgos/java-monorepo".to_owned(),
            private: true,
            admin: Some(true),
        }
    );
    assert_eq!(
        fork_settings,
        PrivateRepoForkWorkflowSettings {
            run_workflows_from_fork_pull_requests: ForkPullRequestWorkflowSetting::Disabled,
            send_write_tokens_to_workflows: false,
            send_secrets_and_variables: false,
            require_approval_for_fork_pr_workflows: false,
        }
    );
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(script.seen[0].path, "repos/ChainArgos/java-monorepo");
    assert_eq!(
        script.seen[1].path,
        "repos/ChainArgos/java-monorepo/actions/permissions/fork-pr-workflows-private-repos"
    );
    for request in &script.seen {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.body, [] as [u8; 0]);
        assert_eq!(request.query, None);
        assert_eq!(header(request, "X-GitHub-Api-Version"), Some("2026-03-10"));
        assert_eq!(
            header(request, "Authorization"),
            Some("Bearer actions-read-canary")
        );
    }
    Ok(())
}

#[test]
fn repository_admin_permission_preserves_missing_and_denied_as_distinct_metadata() {
    let mut denied = Script::replies(&[
        r#"{"id":829618808,"full_name":"ChainArgos/java-monorepo","private":true,"permissions":{"admin":false}}"#,
    ]);
    let denied = get_actions_repository(&mut denied, "ChainArgos", "java-monorepo", TOKEN)
        .expect("repository metadata");
    assert_eq!(denied.admin, Some(false));

    let mut missing = Script::replies(&[
        r#"{"id":829618808,"full_name":"ChainArgos/java-monorepo","private":true}"#,
    ]);
    let missing = get_actions_repository(&mut missing, "ChainArgos", "java-monorepo", TOKEN)
        .expect("repository metadata");
    assert_eq!(missing.admin, None);
}

#[test]
fn trust_reads_reject_missing_fields_and_mismatched_repository() {
    let mut missing_private =
        Script::replies(&[r#"{"id":829618808,"full_name":"ChainArgos/java-monorepo"}"#]);
    assert_eq!(
        get_actions_repository(&mut missing_private, "ChainArgos", "java-monorepo", TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );

    let mut wrong_repo = Script::replies(&[
        r#"{"id":829618808,"full_name":"another-org/java-monorepo","private":true,"permissions":{"admin":true}}"#,
    ]);
    assert_eq!(
        get_actions_repository(&mut wrong_repo, "ChainArgos", "java-monorepo", TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );

    let mut missing_policy = Script::replies(&[
        r#"{"run_workflows_from_fork_pull_requests":false,"send_write_tokens_to_workflows":false,"send_secrets_and_variables":false}"#,
    ]);
    assert_eq!(
        get_private_repo_fork_workflow_settings(
            &mut missing_policy,
            "ChainArgos",
            "java-monorepo",
            TOKEN
        ),
        Err(SessionError::Wire(WireError::Malformed))
    );
}

#[test]
fn missing_workflow_path_is_not_accepted_as_trust_evidence() {
    let mut response = Script::replies(&[
        r#"{"id":88,"run_attempt":1,"status":"completed","event":"push","head_sha":"abc123","head_repository":{"full_name":"ChainArgos/java-monorepo"}}"#,
    ]);
    assert_eq!(
        get_actions_workflow_run(&mut response, "ChainArgos", "java-monorepo", 88, TOKEN),
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
