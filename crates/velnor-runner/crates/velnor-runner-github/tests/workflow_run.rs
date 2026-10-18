//! GitHub workflow and pull request source verification tests.

#[path = "workflow_run/fixtures.rs"]
mod fixtures;

use fixtures::{
    BASE_SHA, HEAD_REPOSITORY_ID, HEAD_SHA, MERGE_SHA, OWNER, REPOSITORY, REPOSITORY_ID, RUN_ID,
    empty_source, pull_request_json, pull_request_run_json, source_metadata, target,
    workflow_run_json,
};

use velnor_runner_github::workflow_run::{
    GITHUB_API_VERSION, PullRequest, PullRequestMismatch, ReferencedWorkflow, RepositoryTarget,
    SourceMismatch, WorkflowRefKind, WorkflowRun, WorkflowRunError, get_pull_request,
    get_workflow_run, verify_pull_request, verify_workflow_source,
};
use velnor_runner_github::{
    CompleteSourceMetadata, Exchange, Method, SessionRequest, SourceMetadata, Transport,
    TransportFail,
};

struct Script {
    reply: Result<Exchange, TransportFail>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn response(body: &str) -> Self {
        Self {
            reply: Ok(Exchange {
                status: 200,
                body: body.as_bytes().to_vec(),
            }),
            seen: Vec::new(),
        }
    }

    fn failure(fail: TransportFail) -> Self {
        Self {
            reply: Err(fail),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.reply.clone()
    }
}

#[test]
fn workflow_run_request_uses_pinned_api_and_returns_source_fields() -> Result<(), &'static str> {
    let mut script = Script::response(&workflow_run_json("push", "sea/integration"));
    let run = get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "token-canary")
        .map_err(|_| "workflow run")?;
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.path,
        "/repos/ChainArgos/java-monorepo/actions/runs/2468"
    );
    assert_eq!(request.query, None);
    assert!(
        request
            .headers
            .iter()
            .any(|(name, value)| { name == "Accept" && value == "application/vnd.github+json" })
    );
    assert!(
        request
            .headers
            .iter()
            .any(|(name, value)| { name == "Authorization" && value == "Bearer token-canary" })
    );
    assert!(
        request
            .headers
            .iter()
            .any(|(name, value)| { name == "X-GitHub-Api-Version" && value == GITHUB_API_VERSION })
    );
    assert_eq!(run.repository.id, REPOSITORY_ID);
    assert_eq!(run.event, "push");
    assert_eq!(run.head_branch.as_deref(), Some("sea/integration"));
    assert_eq!(run.head_sha, HEAD_SHA);
    assert_eq!(
        run.referenced_workflows
            .first()
            .ok_or("referenced workflow")?
            .sha,
        HEAD_SHA
    );
    Ok(())
}

#[test]
fn workflow_source_check_rejects_wrong_run_repo_event_and_ref() -> Result<(), &'static str> {
    let metadata = source_metadata();
    let complete = metadata.complete().ok_or("complete source")?;
    let mut run: WorkflowRun =
        serde_json::from_str(&workflow_run_json("pull_request", "sea/integration"))
            .map_err(|_| "run")?;
    assert_eq!(
        verify_workflow_source(complete, &run, target()),
        Err(SourceMismatch::Event)
    );

    run.event = "push".to_owned();
    let verified = verify_workflow_source(complete, &run, target()).map_err(|_| "source")?;
    assert_eq!(verified.repository_id, REPOSITORY_ID);
    assert_eq!(verified.event_name, "push");
    assert_eq!(verified.head_branch.as_deref(), Some("sea/integration"));
    assert_eq!(verified.workflow_source_sha, None);
    assert_eq!(verified.job_workflow.kind, WorkflowRefKind::Branch);
    assert_eq!(
        verified.job_workflow.reference,
        "refs/heads/sea/integration"
    );
    let mut wrong_branch = run.clone();
    wrong_branch.head_branch = Some("other-branch".to_owned());
    assert_eq!(
        verify_workflow_source(complete, &wrong_branch, target()),
        Err(SourceMismatch::WorkflowRef)
    );
    run.head_sha.clear();
    assert_eq!(
        verify_workflow_source(complete, &run, target()),
        Err(SourceMismatch::WorkflowSha)
    );
    run.head_sha = HEAD_SHA.to_owned();

    assert_eq!(
        verify_workflow_source(
            complete,
            &run,
            RepositoryTarget {
                id: REPOSITORY_ID + 1,
                owner: OWNER,
                name: REPOSITORY,
            }
        ),
        Err(SourceMismatch::RepositoryId)
    );
    run.repository.owner.login = "other-owner".to_owned();
    assert_eq!(
        verify_workflow_source(complete, &run, target()),
        Err(SourceMismatch::RepositoryName)
    );
    run.repository.owner.login = OWNER.to_owned();
    run.path = ".github/workflows/other.yml@sea/integration".to_owned();
    run.referenced_workflows.clear();
    assert_eq!(
        verify_workflow_source(complete, &run, target()),
        Err(SourceMismatch::WorkflowRef)
    );
    run.path = ".github/workflows/ci.yml@refs/heads/sea/integration".to_owned();
    let wrong_run = CompleteSourceMetadata {
        workflow_run_id: RUN_ID + 1,
        ..complete
    };
    assert_eq!(
        verify_workflow_source(wrong_run, &run, target()),
        Err(SourceMismatch::WorkflowRunId)
    );
    Ok(())
}

#[test]
fn workflow_ref_kind_is_preserved_and_repository_is_checked() -> Result<(), &'static str> {
    let metadata = source_metadata();
    let complete = metadata.complete().ok_or("complete source")?;
    let mut run: WorkflowRun =
        serde_json::from_str(&workflow_run_json("push", "sea/integration")).map_err(|_| "run")?;
    let verified = verify_workflow_source(complete, &run, target()).map_err(|_| "branch")?;
    assert_eq!(verified.job_workflow.kind, WorkflowRefKind::Branch);

    let same_name_tag = CompleteSourceMetadata {
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/tags/sea/integration",
        ..complete
    };
    run.path = ".github/workflows/ci.yml@refs/heads/sea/integration".to_owned();
    assert_eq!(
        verify_workflow_source(same_name_tag, &run, target()),
        Err(SourceMismatch::WorkflowRef)
    );
    run.path = ".github/workflows/ci.yml".to_owned();
    let wrong_repository = CompleteSourceMetadata {
        job_workflow_ref: "other-owner/other-repo/.github/workflows/ci.yml@refs/heads/sea/integration",
        ..complete
    };
    assert_eq!(
        verify_workflow_source(wrong_repository, &run, target()),
        Err(SourceMismatch::WorkflowRef)
    );

    run.path = ".github/workflows/caller.yml@refs/heads/sea/integration".to_owned();
    run.referenced_workflows.push(ReferencedWorkflow {
        path: "ChainArgos/java-monorepo/.github/workflows/release.yml@v2".to_owned(),
        sha: HEAD_SHA.to_owned(),
        reference: Some("refs/tags/v2".to_owned()),
    });
    let reusable_tag = CompleteSourceMetadata {
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/release.yml@refs/tags/v2",
        ..complete
    };
    let tag_result = verify_workflow_source(reusable_tag, &run, target());
    assert!(
        tag_result.is_ok(),
        "reusable workflow verification: {tag_result:?}"
    );
    let verified_tag = tag_result.map_err(|_| "reusable workflow tag")?;
    assert_eq!(verified_tag.job_workflow.kind, WorkflowRefKind::Tag);
    assert_eq!(verified_tag.workflow_source_sha.as_deref(), Some(HEAD_SHA));
    run.referenced_workflows[0].path =
        format!("ChainArgos/java-monorepo/.github/workflows/release.yml@{HEAD_SHA}");
    run.referenced_workflows[0].reference = None;
    let pinned_workflow = CompleteSourceMetadata {
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/release.yml@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ..complete
    };
    let verified_pinned =
        verify_workflow_source(pinned_workflow, &run, target()).map_err(|_| "pinned workflow")?;
    assert_eq!(verified_pinned.job_workflow.kind, WorkflowRefKind::Commit);
    assert_eq!(
        verified_pinned.workflow_source_sha.as_deref(),
        Some(HEAD_SHA)
    );
    Ok(())
}

#[test]
fn missing_message_metadata_cannot_create_verified_source() -> Result<(), &'static str> {
    let source = SourceMetadata::default();
    let run: WorkflowRun =
        serde_json::from_str(&workflow_run_json("push", "sea/integration")).map_err(|_| "run")?;
    assert_eq!(source.complete(), None);
    assert_eq!(
        verify_workflow_source(empty_source(), &run, target()),
        Err(SourceMismatch::MissingValue)
    );
    Ok(())
}

#[test]
fn pull_request_check_keeps_run_head_and_merge_identity_separate() -> Result<(), &'static str> {
    let run: WorkflowRun = serde_json::from_str(&pull_request_run_json()).map_err(|_| "run")?;
    let pull: PullRequest = serde_json::from_str(&pull_request_json()).map_err(|_| "pull")?;
    let pr_source = CompleteSourceMetadata {
        owner_name: OWNER,
        repository_name: REPOSITORY,
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge",
        workflow_run_id: RUN_ID,
        event_name: "pull_request",
    };
    let verified_source =
        verify_workflow_source(pr_source, &run, target()).map_err(|_| "PR workflow source")?;
    assert_eq!(
        verified_source.job_workflow.kind,
        WorkflowRefKind::PullRequest
    );
    assert_eq!(verified_source.workflow_source_sha, None);

    let wrong_pr_source = CompleteSourceMetadata {
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2111/merge",
        ..pr_source
    };
    assert_eq!(
        verify_workflow_source(wrong_pr_source, &run, target()),
        Err(SourceMismatch::WorkflowRef)
    );

    let verified = verify_pull_request(&run, &pull, REPOSITORY_ID).map_err(|_| "check")?;
    assert_eq!(verified.number, 2110);
    assert_eq!(verified.head_repository_id, HEAD_REPOSITORY_ID);
    assert_eq!(verified.base_repository_id, REPOSITORY_ID);
    assert_eq!(verified.head_ref, "feature");
    assert_eq!(verified.head_sha, HEAD_SHA);
    assert_eq!(verified.base_ref, "sea/integration");
    assert_eq!(verified.base_sha, BASE_SHA);
    assert_eq!(verified.run_head_sha, HEAD_SHA);
    assert_eq!(verified.merge_commit_sha.as_deref(), Some(MERGE_SHA));

    let mut missing_merge_sha = pull.clone();
    missing_merge_sha.merge_commit_sha = None;
    let without_merge = verify_pull_request(&run, &missing_merge_sha, REPOSITORY_ID)
        .map_err(|_| "optional merge SHA")?;
    assert_eq!(without_merge.run_head_sha, HEAD_SHA);
    assert_eq!(without_merge.merge_commit_sha, None);

    assert_eq!(
        verify_pull_request(&run, &pull, REPOSITORY_ID + 1),
        Err(PullRequestMismatch::BaseRepository)
    );
    let mut wrong_fork = run.clone();
    if let Some(repository) = wrong_fork.head_repository.as_mut() {
        repository.id += 1;
    }
    assert_eq!(
        verify_pull_request(&wrong_fork, &pull, REPOSITORY_ID),
        Err(PullRequestMismatch::HeadRepository)
    );
    let mut wrong_base = pull.clone();
    wrong_base.base.reference = "main".to_owned();
    assert_eq!(
        verify_pull_request(&run, &wrong_base, REPOSITORY_ID),
        Err(PullRequestMismatch::Base)
    );
    let mut wrong_run_head = run.clone();
    wrong_run_head.head_sha = BASE_SHA.to_owned();
    assert_eq!(
        verify_pull_request(&wrong_run_head, &pull, REPOSITORY_ID),
        Err(PullRequestMismatch::Head)
    );
    let mut changed_current_merge = pull;
    changed_current_merge.merge_commit_sha = Some(BASE_SHA.to_owned());
    let changed = verify_pull_request(&run, &changed_current_merge, REPOSITORY_ID)
        .map_err(|_| "separate current merge")?;
    assert_eq!(changed.run_head_sha, HEAD_SHA);
    assert_eq!(changed.merge_commit_sha.as_deref(), Some(BASE_SHA));
    Ok(())
}

#[test]
fn pull_request_request_rejects_invalid_number_before_transport() {
    let mut script = Script::response("{}");
    assert_eq!(
        get_pull_request(&mut script, OWNER, REPOSITORY, 0, "token-canary"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert!(script.seen.is_empty());
}

#[test]
fn pull_request_request_uses_the_exact_repository_path() -> Result<(), &'static str> {
    let mut script = Script::response(&pull_request_json());
    let pull = get_pull_request(&mut script, OWNER, REPOSITORY, 2110, "token-canary")
        .map_err(|_| "pull request")?;
    assert_eq!(pull.number, 2110);
    assert_eq!(pull.base.reference, "sea/integration");
    assert_eq!(
        pull.head.repository.as_ref().map(|repo| repo.id),
        Some(HEAD_REPOSITORY_ID)
    );
    assert_eq!(
        script.seen.first().map(|request| request.path.as_str()),
        Some("/repos/ChainArgos/java-monorepo/pulls/2110")
    );
    Ok(())
}

#[test]
fn api_failures_do_not_create_source_records() {
    let mut script = Script::failure(TransportFail::Timeout);
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "token-canary"),
        Err(WorkflowRunError::Uncertain)
    );
    let mut script = Script {
        reply: Ok(Exchange {
            status: 403,
            body: Vec::new(),
        }),
        seen: Vec::new(),
    };
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "token-canary"),
        Err(WorkflowRunError::HttpStatus(403))
    );
}

#[test]
fn invalid_api_path_and_token_do_not_reach_transport() {
    let mut script = Script::response("{}");
    assert_eq!(
        get_workflow_run(&mut script, "owner/repo", REPOSITORY, RUN_ID, "token"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "bad\ntoken"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert!(script.seen.is_empty());
}
