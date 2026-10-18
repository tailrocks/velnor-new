//! Historical pull request run proof checks.

use std::collections::HashMap;

use velnor_runner_github::workflow_run::{
    BaseRefChangedEvent, GitCommitIdentity, PullRequestBaseHistory, PullRequestExecutionContext,
    PullRequestExecutionError, PullRequestExecutionMismatch, PullRequestNoncePublication,
    PullRequestRetargetAcknowledgement, RepositoryTarget, WorkflowRun, WorkflowRunError,
    get_git_commit, verify_pull_request_execution, verify_workflow_source,
};
use velnor_runner_github::{
    CompleteSourceMetadata, Exchange, SessionRequest, Transport, TransportFail,
};

const OWNER: &str = "ChainArgos";
const REPOSITORY: &str = "java-monorepo";
const REPOSITORY_ID: i64 = 41;
const HEAD_REPOSITORY_ID: i64 = 73;
const PR_NUMBER: i64 = 2110;
const PRIOR_HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACK_BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NONCE_HEAD: &str = "cccccccccccccccccccccccccccccccccccccccc";
const RUN_BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const CURRENT_BASE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const RUN_MERGE: &str = "ffffffffffffffffffffffffffffffffffffffff";
const TREE: &str = "9999999999999999999999999999999999999999";
const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const AUTHOR_NAME: &str = "Configured Person";
const AUTHOR_EMAIL: &str = "person@example.com";
const SIGNED_OFF_NONCE_MESSAGE: &str = "Add validation nonce\n\nvelnor-pr-validation-nonce:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n\nSigned-off-by: Configured Person <person@example.com>";

fn identity() -> Result<GitCommitIdentity, String> {
    GitCommitIdentity::try_new(AUTHOR_NAME, AUTHOR_EMAIL).map_err(|error| error.to_string())
}

#[derive(Default)]
struct CommitApi {
    commits: HashMap<String, String>,
    requests: Vec<SessionRequest>,
}

impl Transport for CommitApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(request.clone());
        let sha = request
            .path
            .rsplit('/')
            .next()
            .ok_or(TransportFail::Reset)?;
        match self.commits.get(sha) {
            Some(body) => Ok(Exchange {
                status: 200,
                body: body.as_bytes().to_vec(),
            }),
            None => Ok(Exchange {
                status: 404,
                body: b"{}".to_vec(),
            }),
        }
    }
}

fn commit_json(sha: &str, message: &str, parents: &[&str]) -> String {
    let parents = parents
        .iter()
        .map(|parent| serde_json::json!({"sha": parent}))
        .collect::<Vec<_>>();
    serde_json::json!({
        "sha": sha,
        "message": message,
        "author": {"name": AUTHOR_NAME, "email": AUTHOR_EMAIL},
        "committer": {"name": AUTHOR_NAME, "email": AUTHOR_EMAIL},
        "tree": {"sha": TREE},
        "parents": parents
    })
    .to_string()
}

fn context() -> Result<(WorkflowRun, PullRequestBaseHistory, CommitApi), String> {
    let run_json = format!(
        r#"{{"id":2468,"event":"pull_request","path":".github/workflows/ci.yml","head_branch":"feature","head_sha":"{RUN_MERGE}","workflow_id":17,"run_attempt":1,"repository":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","private":true,"owner":{{"login":"{OWNER}"}}}},"head_repository":{{"id":{HEAD_REPOSITORY_ID},"name":"fork-repo","full_name":"fork-owner/fork-repo","owner":{{"login":"fork-owner"}}}},"pull_requests":[{{"number":{PR_NUMBER},"head":{{"ref":"mutable-feature","sha":"1111111111111111111111111111111111111111","repo":{{"id":{HEAD_REPOSITORY_ID},"name":"fork-repo"}}}},"base":{{"ref":"mutable-base","sha":"2222222222222222222222222222222222222222","repo":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}"}}}}}}]}}"#
    );
    let run: WorkflowRun = serde_json::from_str(&run_json).map_err(|error| error.to_string())?;
    let history = PullRequestBaseHistory {
        number: PR_NUMBER,
        repository_id: REPOSITORY_ID,
        head_repository_id: HEAD_REPOSITORY_ID,
        head_ref: "feature".to_owned(),
        head_sha: NONCE_HEAD.to_owned(),
        base_repository_id: REPOSITORY_ID,
        base_ref: "sea/integration".to_owned(),
        base_sha: CURRENT_BASE.to_owned(),
        updated_at: "2026-10-18T10:00:00Z".to_owned(),
        events: vec![BaseRefChangedEvent {
            id: "base-event-42".to_owned(),
            database_id: Some(42),
            created_at: "2026-10-18T09:00:00Z".to_owned(),
            previous_ref_name: "main".to_owned(),
            current_ref_name: "sea/integration".to_owned(),
        }],
    };
    let mut api = CommitApi::default();
    api.commits.insert(
        NONCE_HEAD.to_owned(),
        commit_json(NONCE_HEAD, SIGNED_OFF_NONCE_MESSAGE, &[PRIOR_HEAD]),
    );
    api.commits.insert(
        RUN_MERGE.to_owned(),
        commit_json(RUN_MERGE, "Synthetic merge", &[RUN_BASE, NONCE_HEAD]),
    );
    api.commits.insert(
        RUN_BASE.to_owned(),
        commit_json(RUN_BASE, "Base at run", &[ACK_BASE]),
    );
    api.commits.insert(
        CURRENT_BASE.to_owned(),
        commit_json(CURRENT_BASE, "Current base", &[RUN_BASE]),
    );
    Ok((run, history, api))
}

fn source(
    run: &WorkflowRun,
) -> Result<velnor_runner_github::workflow_run::VerifiedWorkflowSource, String> {
    verify_workflow_source(
        CompleteSourceMetadata {
            owner_name: OWNER,
            repository_name: REPOSITORY,
            job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge",
            workflow_run_id: run.id,
            event_name: "pull_request",
        },
        run,
        RepositoryTarget {
            id: REPOSITORY_ID,
            owner: OWNER,
            name: REPOSITORY,
        },
    )
    .map_err(|error| error.to_string())
}

fn acknowledgement() -> PullRequestRetargetAcknowledgement {
    PullRequestRetargetAcknowledgement {
        number: PR_NUMBER,
        base_repository_id: REPOSITORY_ID,
        head_repository_id: HEAD_REPOSITORY_ID,
        head_ref: "feature".to_owned(),
        head_sha: PRIOR_HEAD.to_owned(),
        previous_base_ref: "main".to_owned(),
        current_base_ref: "sea/integration".to_owned(),
        event_id: "base-event-42".to_owned(),
        base_sha: ACK_BASE.to_owned(),
    }
}

fn publication() -> Result<PullRequestNoncePublication, String> {
    Ok(PullRequestNoncePublication {
        nonce: NONCE.to_owned(),
        commit_sha: NONCE_HEAD.to_owned(),
        identity: identity()?,
    })
}

fn verify(
    run: &WorkflowRun,
    history: &PullRequestBaseHistory,
    api: &mut CommitApi,
    ack: &PullRequestRetargetAcknowledgement,
    nonce: &PullRequestNoncePublication,
) -> Result<
    velnor_runner_github::workflow_run::VerifiedPullRequestExecution,
    PullRequestExecutionError,
> {
    let source = source(run).map_err(|_| PullRequestExecutionMismatch::WorkflowSource)?;
    verify_pull_request_execution(
        api,
        run,
        &source,
        PullRequestExecutionContext {
            expected: RepositoryTarget {
                id: REPOSITORY_ID,
                owner: OWNER,
                name: REPOSITORY,
            },
            expected_head_repository_id: HEAD_REPOSITORY_ID,
            expected_head_ref: "feature",
            expected_base_ref: "sea/integration",
            history,
            acknowledgement: ack,
            publication: nonce,
        },
        "token",
    )
}

#[test]
fn proves_nonce_commit_and_exact_run_merge_ancestry_despite_mutable_pr_snapshot()
-> Result<(), String> {
    let (run, history, mut api) = context()?;
    let proof = verify(
        &run,
        &history,
        &mut api,
        &acknowledgement(),
        &publication()?,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(proof.association().number, PR_NUMBER);
    assert_eq!(proof.run_base_sha(), Some(RUN_BASE));
    assert_eq!(proof.retarget_event_id(), "base-event-42");
    assert_eq!(proof.nonce_commit_sha(), NONCE_HEAD);
    assert_eq!(api.requests.len(), 4);
    Ok(())
}

#[test]
fn rejects_later_base_retarget_and_wrong_acknowledgement() -> Result<(), String> {
    let (run, mut history, mut api) = context()?;
    history.events.push(BaseRefChangedEvent {
        id: "later-event".to_owned(),
        database_id: Some(43),
        created_at: "2026-10-18T09:30:00Z".to_owned(),
        previous_ref_name: "sea/integration".to_owned(),
        current_ref_name: "main".to_owned(),
    });
    history.base_ref = "main".to_owned();
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::BaseHistory
        ))
    ));

    let (run, history, mut api) = context()?;
    let mut ack = acknowledgement();
    ack.event_id = "wrong-event".to_owned();
    assert!(matches!(
        verify(&run, &history, &mut api, &ack, &publication()?),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::BaseHistory
        ))
    ));
    Ok(())
}

#[test]
fn rejects_nonce_without_post_ack_parent_and_run_not_using_nonce() -> Result<(), String> {
    let (run, history, mut api) = context()?;
    api.commits.insert(
        NONCE_HEAD.to_owned(),
        commit_json(
            NONCE_HEAD,
            &format!("velnor-pr-validation-nonce:v1:{NONCE}"),
            &[ACK_BASE],
        ),
    );
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::NonceCommit
        ))
    ));

    let (mut run, history, mut api) = context()?;
    api.commits.insert(
        RUN_MERGE.to_owned(),
        commit_json(RUN_MERGE, "Synthetic merge", &[RUN_BASE, PRIOR_HEAD]),
    );
    run.head_sha = RUN_MERGE.to_owned();
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::RunCommit
        ))
    ));
    Ok(())
}

#[test]
fn rejects_run_source_and_base_graph_from_a_different_context() -> Result<(), String> {
    let (mut run, history, mut api) = context()?;
    run.pull_requests[0].number += 1;
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));

    let (run, history, mut api) = context()?;
    api.commits.insert(
        CURRENT_BASE.to_owned(),
        commit_json(CURRENT_BASE, "rewritten base", &[]),
    );
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::BaseAncestry
        ))
    ));

    let (mut old_run, history, mut api) = context()?;
    old_run.head_sha = PRIOR_HEAD.to_owned();
    api.commits.insert(
        PRIOR_HEAD.to_owned(),
        commit_json(PRIOR_HEAD, "Pre-retarget workflow", &[ACK_BASE]),
    );
    assert!(matches!(
        verify(
            &old_run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::RunCommit
        ))
    ));

    let (mut wrong_repo, history, mut api) = context()?;
    wrong_repo.repository.id += 1;
    assert!(matches!(
        verify(
            &wrong_repo,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));

    let (mut wrong_fork, history, mut api) = context()?;
    if let Some(head_repository) = wrong_fork.head_repository.as_mut() {
        head_repository.id += 1;
    }
    assert!(matches!(
        verify(
            &wrong_fork,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));
    Ok(())
}

#[test]
fn commit_lookup_from_execution_uses_exact_repository_and_rejects_absent_objects() {
    let mut api = CommitApi::default();
    assert_eq!(
        get_git_commit(&mut api, OWNER, REPOSITORY, NONCE_HEAD, "token"),
        Err(WorkflowRunError::HttpStatus(404))
    );
}

#[test]
fn direct_head_run_requires_the_same_nonce_publication() -> Result<(), String> {
    let (mut run, history, mut api) = context()?;
    run.head_sha = NONCE_HEAD.to_owned();
    let source = source(&run)?;
    let proof = verify_pull_request_execution(
        &mut api,
        &run,
        &source,
        PullRequestExecutionContext {
            expected: RepositoryTarget {
                id: REPOSITORY_ID,
                owner: OWNER,
                name: REPOSITORY,
            },
            expected_head_repository_id: HEAD_REPOSITORY_ID,
            expected_head_ref: "feature",
            expected_base_ref: "sea/integration",
            history: &history,
            acknowledgement: &acknowledgement(),
            publication: &publication()?,
        },
        "token",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(proof.run_base_sha(), None);
    Ok(())
}
