//! Actions workflow trust DTO tests.

use velnor_runner_github::{
    Exchange, Method, SessionError, SessionRequest, Transport, TransportFail, WireError,
    policy::{WorkflowTrustField, get_actions_workflow_trust_run},
};

const TOKEN: &str = "actions-trust-read-test-token";

struct Script {
    result: Option<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn json(body: &str) -> Self {
        Self {
            result: Some(Ok(Exchange {
                status: 200,
                body: body.as_bytes().to_vec(),
            })),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.result.take().unwrap_or(Err(TransportFail::Reset))
    }
}

#[test]
fn trust_run_read_preserves_exact_rest_references_and_uses_one_get() {
    let mut script = Script::json(
        r#"{"id":88,"run_attempt":2,"status":"queued","event":"push","path":".github/workflows/ci.yml@main","head_sha":"0123456789abcdef0123456789abcdef01234567","head_branch":"main","head_repository":{"full_name":"ChainArgos/java-monorepo"},"referenced_workflows":[{"path":"ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1","ref":"refs/tags/v1","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
    );
    let run = get_actions_workflow_trust_run(&mut script, "ChainArgos", "java-monorepo", 88, TOKEN)
        .expect("read run");

    assert_eq!(run.id(), 88);
    assert_eq!(run.observed_run_attempt(), 2);
    assert_eq!(run.event(), "push");
    assert_eq!(run.path(), ".github/workflows/ci.yml@main");
    assert_eq!(
        run.head_branch(),
        &WorkflowTrustField::Present("main".to_owned())
    );
    assert_eq!(
        run.head_repository_full_name(),
        &WorkflowTrustField::Present("ChainArgos/java-monorepo".to_owned())
    );
    let WorkflowTrustField::Present(workflows) = run.referenced_workflows() else {
        panic!("referenced workflows should be present");
    };
    assert_eq!(workflows.len(), 1);
    assert_eq!(
        workflows[0].path(),
        "ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1"
    );
    assert_eq!(
        workflows[0].git_ref(),
        &WorkflowTrustField::Present("refs/tags/v1".to_owned())
    );
    assert_eq!(
        workflows[0].sha(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(
        script.seen[0].path,
        "repos/ChainArgos/java-monorepo/actions/runs/88"
    );
    assert_eq!(script.seen[0].body, [] as [u8; 0]);
    assert!(!format!("{:?}", script.seen[0]).contains(TOKEN));
}

#[test]
fn missing_null_and_malformed_trust_fields_remain_distinct() {
    let mut missing = Script::json(
        r#"{"id":88,"run_attempt":1,"event":"push","path":".github/workflows/ci.yml@main","head_sha":"abc123"}"#,
    );
    let run = get_actions_workflow_trust_run(&mut missing, "org", "repo", 88, TOKEN)
        .expect("missing optional metadata is retained");
    assert_eq!(run.head_branch(), &WorkflowTrustField::Missing);
    assert_eq!(
        run.head_repository_full_name(),
        &WorkflowTrustField::Missing
    );
    assert_eq!(run.referenced_workflows(), &WorkflowTrustField::Missing);

    let mut invalid = Script::json(
        r#"{"id":88,"run_attempt":1,"event":"push","path":".github/workflows/ci.yml@main","head_sha":"abc123","head_branch":17,"head_repository":[],"referenced_workflows":[17]}"#,
    );
    let run = get_actions_workflow_trust_run(&mut invalid, "org", "repo", 88, TOKEN)
        .expect("invalid optional metadata is retained");
    assert_eq!(run.head_branch(), &WorkflowTrustField::Invalid);
    assert_eq!(
        run.head_repository_full_name(),
        &WorkflowTrustField::Invalid
    );
    assert_eq!(run.referenced_workflows(), &WorkflowTrustField::Invalid);
}

#[test]
fn missing_reusable_ref_is_not_silently_normalized() {
    let mut script = Script::json(
        r#"{"id":88,"run_attempt":1,"event":"push","path":".github/workflows/ci.yml@main","head_sha":"abc123","head_branch":"main","head_repository":{"full_name":"org/repo"},"referenced_workflows":[{"path":"org/repo/.github/workflows/reuse.yml@main","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
    );
    let run =
        get_actions_workflow_trust_run(&mut script, "org", "repo", 88, TOKEN).expect("run parse");
    let WorkflowTrustField::Present(workflows) = run.referenced_workflows() else {
        panic!("workflow list should be present");
    };
    assert_eq!(workflows[0].git_ref(), &WorkflowTrustField::Missing);
}

#[test]
fn wrong_run_identity_and_bad_required_fields_fail_closed() {
    let mut wrong_id = Script::json(
        r#"{"id":89,"run_attempt":1,"event":"push","path":".github/workflows/ci.yml@main","head_sha":"abc123"}"#,
    );
    assert_eq!(
        get_actions_workflow_trust_run(&mut wrong_id, "org", "repo", 88, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );

    let mut bad_path =
        Script::json(r#"{"id":88,"run_attempt":1,"event":"push","path":[],"head_sha":"abc123"}"#);
    assert_eq!(
        get_actions_workflow_trust_run(&mut bad_path, "org", "repo", 88, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
}

#[test]
fn invalid_arguments_fail_before_transport() {
    let mut script = Script::json("{}");
    assert_eq!(
        get_actions_workflow_trust_run(&mut script, "../org", "repo", 88, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        get_actions_workflow_trust_run(&mut script, "org", "repo", 0, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.as_slice(), &[]);
}
