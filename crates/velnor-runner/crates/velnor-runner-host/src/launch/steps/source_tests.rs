use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

use crate::config::{
    ConnectionId, HostConfig, PullRequestRule, SourcePolicy, SourcePolicyMode, WorkflowEvent,
};
use crate::config_test_support::{basic_connection, host_config};
use crate::connect::SetOwnership;
use crate::launch_harness;
use velnor_runner_github::SourceMetadata;

use super::{SourceBlock, authorize_policy};
use api_fixture::SourceApiFixture;

#[path = "source_tests/api_fixture.rs"]
mod api_fixture;
#[path = "source_tests/pull_request.rs"]
mod pull_request;

fn policy(events: Vec<WorkflowEvent>) -> Result<SourcePolicy, crate::error::HostError> {
    let tag_prefixes = if events
        .iter()
        .any(|event| matches!(event, WorkflowEvent::Push | WorkflowEvent::Release))
    {
        vec!["v".to_owned()]
    } else {
        Vec::new()
    };
    let pull_request_rules = if events.contains(&WorkflowEvent::PullRequest) {
        vec![PullRequestRule {
            number: Some(2110),
            base_repository_id: 77,
            base_branch: "sea/integration".to_owned(),
            head_repository_id: 77,
            head_branch: "fix/runner".to_owned(),
        }]
    } else {
        Vec::new()
    };
    SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["sea/integration".to_owned()],
        tag_prefixes,
        events,
        pull_request_rules,
    )
}

fn source(kind: WorkflowRefKind, reference: &str, event_name: &str) -> VerifiedWorkflowSource {
    let head_branch = if kind == WorkflowRefKind::Tag {
        reference.strip_prefix("refs/tags/").unwrap_or(reference)
    } else {
        "sea/integration"
    };
    VerifiedWorkflowSource {
        repository_id: 77,
        owner_name: "chainargos".to_owned(),
        repository_name: "java-monorepo".to_owned(),
        workflow_run_id: 900,
        event_name: event_name.to_owned(),
        head_branch: Some(head_branch.to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: format!("ChainArgos/java-monorepo/.github/workflows/ci.yml@{reference}"),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: format!("ChainArgos/java-monorepo/.github/workflows/ci.yml@{reference}"),
            owner_name: "ChainArgos".to_owned(),
            repository_name: "java-monorepo".to_owned(),
            workflow_path: ".github/workflows/ci.yml".to_owned(),
            reference: reference.to_owned(),
            kind,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

#[test]
fn push_requires_the_exact_allowed_branch_and_event() -> Result<(), String> {
    let policy = policy(vec![WorkflowEvent::Push]).map_err(|error| error.to_string())?;
    let allowed = source(
        WorkflowRefKind::Branch,
        "refs/heads/sea/integration",
        "push",
    );
    if authorize_policy(&policy, &allowed).is_err() {
        return Err("allowed branch was blocked".to_owned());
    }
    let main = source(WorkflowRefKind::Branch, "refs/heads/main", "push");
    assert_eq!(
        authorize_policy(&policy, &main),
        Err(SourceBlock::RefDenied)
    );
    let wrong_event = source(
        WorkflowRefKind::Branch,
        "refs/heads/sea/integration",
        "schedule",
    );
    assert_eq!(
        authorize_policy(&policy, &wrong_event),
        Err(SourceBlock::EventDenied)
    );
    Ok(())
}

#[test]
fn tags_require_an_allowed_prefix_and_matching_run_branch() -> Result<(), String> {
    let policy = policy(vec![WorkflowEvent::Release]).map_err(|error| error.to_string())?;
    let allowed = source(WorkflowRefKind::Tag, "refs/tags/v1.2.3", "release");
    assert_eq!(authorize_policy(&policy, &allowed), Ok(()));
    let denied = source(WorkflowRefKind::Tag, "refs/tags/x1.2.3", "release");
    assert_eq!(
        authorize_policy(&policy, &denied),
        Err(SourceBlock::RefDenied)
    );
    Ok(())
}

#[test]
fn pull_requests_require_an_authenticated_pull_ref() -> Result<(), String> {
    let policy = policy(vec![WorkflowEvent::PullRequest]).map_err(|error| error.to_string())?;
    let allowed = source(
        WorkflowRefKind::PullRequest,
        "refs/pull/2110/merge",
        "pull_request",
    );
    assert_eq!(authorize_policy(&policy, &allowed), Ok(()));
    let branch = source(
        WorkflowRefKind::Branch,
        "refs/heads/sea/integration",
        "pull_request",
    );
    assert_eq!(
        authorize_policy(&policy, &branch),
        Err(SourceBlock::RefDenied)
    );
    Ok(())
}

#[test]
fn repository_mode_allows_only_listed_events_after_source_verification() -> Result<(), String> {
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::PullRequest, WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    let pull = source(
        WorkflowRefKind::PullRequest,
        "refs/pull/2110/merge",
        "pull_request",
    );
    assert_eq!(authorize_policy(&policy, &pull), Ok(()));
    let denied = source(WorkflowRefKind::Branch, "refs/heads/main", "schedule");
    assert_eq!(
        authorize_policy(&policy, &denied),
        Err(SourceBlock::EventDenied)
    );
    Ok(())
}

#[tokio::test]
async fn authorization_binding_rejects_changed_policy_before_source_lookup() -> Result<(), String> {
    let encoded = host_config(
        &[basic_connection(
            "source-binding",
            77,
            "chainargos/java-monorepo",
            "source-binding",
            "integration",
        )],
        1,
    );
    let config = HostConfig::parse(&encoded).map_err(|error| error.to_string())?;
    let original = config
        .connections()
        .first()
        .ok_or_else(|| "connection fixture missing".to_owned())?;
    let mut changed = original.clone();
    changed.source_policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;

    let (_scratch, journal) = launch_harness::open("source-binding").await?;
    let scope = journal
        .connection_scope(original)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_scale_set(501, original.scale_set.name(), SetOwnership::Created)
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let mut transport = SourceApiFixture {
        authorization: None,
        authorizations: Vec::new(),
        pull_request_responses: false,
        paths: Vec::new(),
    };

    assert_eq!(
        super::authorize_job_with_transport(
            &changed,
            &binding,
            &velnor_runner_github::SourceMetadata::default(),
            44,
            7,
            "test-token",
            &mut transport,
        ),
        Err(SourceBlock::SourceMismatch)
    );
    let mut different_connection = original.clone();
    different_connection.connection_id =
        ConnectionId::parse("different-source-binding").map_err(|error| error.to_string())?;
    assert_eq!(
        super::authorize_job_with_transport(
            &different_connection,
            &binding,
            &velnor_runner_github::SourceMetadata::default(),
            44,
            7,
            "test-token",
            &mut transport,
        ),
        Err(SourceBlock::SourceMismatch)
    );
    assert_eq!(transport.paths, [] as [String; 0]);
    Ok(())
}

#[test]
fn source_verifier_sends_only_the_connection_pat_to_github() -> Result<(), String> {
    let encoded = host_config(
        &[basic_connection(
            "source-auth-boundary",
            77,
            "chainargos/java-monorepo",
            "source-auth-boundary",
            "integration",
        )],
        1,
    );
    let config = HostConfig::parse(&encoded).map_err(|error| error.to_string())?;
    let connection = config
        .connections()
        .first()
        .ok_or_else(|| "connection fixture missing".to_owned())?;
    let source = SourceMetadata {
        owner_name: Some("chainargos".to_owned()),
        repository_name: Some("java-monorepo".to_owned()),
        job_workflow_ref: Some(
            "chainargos/java-monorepo/.github/workflows/ci.yml@refs/heads/sea/integration"
                .to_owned(),
        ),
        workflow_run_id: Some(900),
        event_name: Some("push".to_owned()),
    };
    let mut transport = SourceApiFixture {
        authorization: None,
        authorizations: Vec::new(),
        pull_request_responses: false,
        paths: Vec::new(),
    };
    let complete = source
        .complete()
        .ok_or_else(|| "source metadata incomplete".to_owned())?;
    let (owner, repository) = super::split_repository(&connection.repository)
        .map_err(|error| format!("repository invalid: {error:?}"))?;
    let run = super::get_workflow_run(
        &mut transport,
        owner,
        repository,
        complete.workflow_run_id,
        "connection-pat",
    )
    .map_err(|error| format!("workflow lookup failed: {error:?}"))?;
    let verified = super::verify_workflow_source(
        complete,
        &run,
        super::RepositoryTarget {
            id: connection.repository_id,
            owner,
            name: repository,
        },
    )
    .map_err(|error| format!("workflow verification failed: {error:?}"))?;
    super::authorize_policy(&connection.source_policy, &verified)
        .map_err(|error| format!("source policy failed: {error:?}"))?;

    assert_eq!(
        transport.authorization.as_deref(),
        Some("Bearer connection-pat")
    );
    assert_ne!(
        transport.authorization.as_deref(),
        Some("Bearer scale-set-bearer")
    );
    assert_ne!(
        transport.authorization.as_deref(),
        Some("Bearer queue-bearer")
    );
    Ok(())
}
