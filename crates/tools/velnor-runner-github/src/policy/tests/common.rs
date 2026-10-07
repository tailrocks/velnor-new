use std::sync::LazyLock;

use crate::policy::{
    ActionsWorkflowTrustRun, JobTrustPolicyView, JobTrustRuleView, ParsedTrustBatch,
    ReusableWorkflowRuleView, WorkflowTrustField, parse_poll_with_trust,
};

pub(super) const TRUSTED_WORKFLOW_REF: &str =
    "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main";
pub(super) const REST_WORKFLOW_PATH: &str = ".github/workflows/ci.yml@main";
pub(super) const POLICY_DIGEST: &str = "sha256:policy-1";

pub(super) fn offer(request_id: i64, job_workflow_ref: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "messageType": "JobAvailable",
        "runnerRequestId": request_id,
        "jobId": format!("opaque:{request_id}"),
        "workflowRunId": 88,
        "ownerName": "ChainArgos",
        "repositoryName": "java-monorepo",
        "eventName": "push",
        "requestLabels": ["ubuntu-24.04-scale-set"]
    });
    if let Some(workflow_ref) = job_workflow_ref {
        value["jobWorkflowRef"] = serde_json::Value::String(workflow_ref.to_owned());
    }
    value
}

pub(super) fn batch(message_id: i64, jobs: &[serde_json::Value]) -> ParsedTrustBatch {
    let inner = serde_json::to_string(&jobs).expect("inner jobs serialize");
    let body = serde_json::json!({
        "messageId": message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": inner
    })
    .to_string();
    let crate::policy::PollWithTrust::Batch(parsed) =
        parse_poll_with_trust(200, &body).expect("valid poll")
    else {
        panic!("expected poll batch");
    };
    parsed
}

pub(super) fn valid_run() -> ActionsWorkflowTrustRun {
    ActionsWorkflowTrustRun {
        id: 88,
        observed_run_attempt: 2,
        event: "push".to_owned(),
        path: REST_WORKFLOW_PATH.to_owned(),
        head_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        head_branch: WorkflowTrustField::Present("main".to_owned()),
        head_repository_full_name: WorkflowTrustField::Present(
            "ChainArgos/java-monorepo".to_owned(),
        ),
        referenced_workflows: WorkflowTrustField::Present(Vec::new()),
    }
}

pub(super) fn policy_view<'a>(rules: &'a [JobTrustRuleView<'a>]) -> JobTrustPolicyView<'a> {
    static REPOSITORIES: LazyLock<[String; 1]> =
        LazyLock::new(|| [String::from("ChainArgos/java-monorepo")]);
    static EVENTS: LazyLock<[String; 1]> = LazyLock::new(|| [String::from("push")]);
    static PATHS: LazyLock<[String; 1]> =
        LazyLock::new(|| [String::from(".github/workflows/ci.yml")]);
    static BRANCHES: LazyLock<[String; 1]> = LazyLock::new(|| [String::from("main")]);
    JobTrustPolicyView {
        repository_full_name: "ChainArgos/java-monorepo",
        allowed_repositories: &*REPOSITORIES,
        allowed_events: &*EVENTS,
        allowed_workflow_paths: &*PATHS,
        allowed_head_branches: &*BRANCHES,
        workflow_rules: rules,
        allow_forks: false,
        policy_digest: POLICY_DIGEST,
    }
}

pub(super) fn valid_rule<'a>(refs: &'a [ReusableWorkflowRuleView<'a>]) -> JobTrustRuleView<'a> {
    JobTrustRuleView {
        workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@main",
        job_workflow_ref: TRUSTED_WORKFLOW_REF,
        workflow_path: REST_WORKFLOW_PATH,
        event: "push",
        head_branch: "main",
        referenced_workflows: refs,
    }
}
