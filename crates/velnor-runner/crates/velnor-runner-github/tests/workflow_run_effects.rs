//! Durable PR transition HTTP effect checks. These tests use no live GitHub API.

use std::collections::VecDeque;

use velnor_runner_github::workflow_run::{
    GitCommit, GitCommitIdentity, GitCommitParent, GitCommitPerson, GitCommitTree,
    PullRequestBaseUpdateRequest, PullRequestNonceCommitReceipt, RepositoryTarget,
    WorkflowRunError, create_pull_request_nonce_commit, publish_pull_request_nonce_commit,
    update_pull_request_base,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const OWNER: &str = "ChainArgos";
const BASE_REPOSITORY: &str = "java-monorepo";
const HEAD_OWNER: &str = "fork-owner";
const HEAD_REPOSITORY: &str = "fork-repo";
const BASE_REPOSITORY_ID: i64 = 41;
const HEAD_REPOSITORY_ID: i64 = 73;
const PR_NUMBER: i64 = 2110;
const OLD_HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OLD_BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NEW_BASE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const NONCE_COMMIT: &str = "dddddddddddddddddddddddddddddddddddddddd";
const TREE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const AUTHOR_NAME: &str = "Configured Person";
const AUTHOR_EMAIL: &str = "person@example.com";
const COMMIT_MESSAGE_WITH_SIGNOFF: &str = "Velnor pull request validation\n\nvelnor-pr-validation-nonce:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n\nSigned-off-by: Configured Person <person@example.com>";

fn identity() -> Result<GitCommitIdentity, String> {
    GitCommitIdentity::try_new(AUTHOR_NAME, AUTHOR_EMAIL).map_err(|error| error.to_string())
}

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn responses(responses: &[(u16, String)]) -> Self {
        Self {
            replies: responses
                .iter()
                .map(|(status, body)| {
                    Ok(Exchange {
                        status: *status,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }

    fn with_failure(responses: &[(u16, String)], failure: TransportFail) -> Self {
        let mut script = Self::responses(responses);
        script.replies.push_back(Err(failure));
        script
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

fn base_target() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: BASE_REPOSITORY_ID,
        owner: OWNER,
        name: BASE_REPOSITORY,
    }
}

fn head_target() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: HEAD_REPOSITORY_ID,
        owner: HEAD_OWNER,
        name: HEAD_REPOSITORY,
    }
}

fn repository(id: i64, owner: &str, name: &str, private: bool) -> String {
    serde_json::json!({
        "id": id,
        "name": name,
        "full_name": format!("{owner}/{name}"),
        "private": private,
        "owner": {"login": owner}
    })
    .to_string()
}

fn pull_json(base_ref: &str, base_sha: &str, head_sha: &str) -> String {
    serde_json::json!({
        "number": PR_NUMBER,
        "state": "open",
        "head": {
            "ref": "feature",
            "sha": head_sha,
            "repo": {
                "id": HEAD_REPOSITORY_ID,
                "name": HEAD_REPOSITORY,
                "full_name": format!("{HEAD_OWNER}/{HEAD_REPOSITORY}"),
                "private": false,
                "owner": {"login": HEAD_OWNER}
            }
        },
        "base": {
            "ref": base_ref,
            "sha": base_sha,
            "repo": {
                "id": BASE_REPOSITORY_ID,
                "name": BASE_REPOSITORY,
                "full_name": format!("{OWNER}/{BASE_REPOSITORY}"),
                "private": true,
                "owner": {"login": OWNER}
            }
        },
        "merge_commit_sha": null
    })
    .to_string()
}

fn commit_json(sha: &str, message: &str, tree: &str, parents: &[&str]) -> String {
    let parents = parents
        .iter()
        .map(|parent| serde_json::json!({"sha": parent}))
        .collect::<Vec<_>>();
    serde_json::json!({
        "sha": sha,
        "message": message,
        "author": {"name": AUTHOR_NAME, "email": AUTHOR_EMAIL},
        "committer": {"name": AUTHOR_NAME, "email": AUTHOR_EMAIL},
        "tree": {"sha": tree},
        "parents": parents
    })
    .to_string()
}

fn reference_json(branch: &str, sha: &str) -> String {
    serde_json::json!({
        "ref": format!("refs/heads/{branch}"),
        "object": {"type": "commit", "sha": sha}
    })
    .to_string()
}

fn base_request() -> PullRequestBaseUpdateRequest {
    PullRequestBaseUpdateRequest {
        number: PR_NUMBER,
        head_repository_id: HEAD_REPOSITORY_ID,
        head_ref: "feature".to_owned(),
        head_sha: OLD_HEAD.to_owned(),
        previous_base_ref: "main".to_owned(),
        previous_base_sha: OLD_BASE.to_owned(),
        target_base_ref: "sea/integration".to_owned(),
    }
}

fn nonce_receipt() -> Result<PullRequestNonceCommitReceipt, String> {
    Ok(PullRequestNonceCommitReceipt {
        head_repository_id: HEAD_REPOSITORY_ID,
        head_ref: "feature/review".to_owned(),
        parent_sha: OLD_HEAD.to_owned(),
        nonce: NONCE.to_owned(),
        identity: identity()?,
        commit: GitCommit {
            sha: NONCE_COMMIT.to_owned(),
            message: COMMIT_MESSAGE_WITH_SIGNOFF.to_owned(),
            author: Some(GitCommitPerson {
                name: AUTHOR_NAME.to_owned(),
                email: AUTHOR_EMAIL.to_owned(),
            }),
            committer: Some(GitCommitPerson {
                name: AUTHOR_NAME.to_owned(),
                email: AUTHOR_EMAIL.to_owned(),
            }),
            tree: GitCommitTree {
                sha: TREE.to_owned(),
            },
            parents: vec![GitCommitParent {
                sha: OLD_HEAD.to_owned(),
            }],
        },
    })
}

fn commit_responses() -> Vec<(u16, String)> {
    vec![
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (200, commit_json(OLD_HEAD, "Prior head", TREE, &[])),
        (
            201,
            commit_json(NONCE_COMMIT, COMMIT_MESSAGE_WITH_SIGNOFF, TREE, &[OLD_HEAD]),
        ),
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
    ]
}

#[test]
fn retarget_requires_exact_preflight_and_sends_one_patch() -> Result<(), String> {
    let responses = [
        (
            200,
            repository(BASE_REPOSITORY_ID, OWNER, BASE_REPOSITORY, true),
        ),
        (200, pull_json("main", OLD_BASE, OLD_HEAD)),
        (200, pull_json("sea/integration", NEW_BASE, OLD_HEAD)),
        (
            200,
            repository(BASE_REPOSITORY_ID, OWNER, BASE_REPOSITORY, true),
        ),
    ];
    let mut script = Script::responses(&responses);
    let receipt = update_pull_request_base(&mut script, base_target(), base_request(), "token")
        .map_err(|error| error.to_string())?;
    assert_eq!(receipt.target_base_sha(), NEW_BASE);
    assert_eq!(script.seen.len(), 4);
    let request = script.seen.get(2).ok_or("PATCH request")?;
    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.path, "/repos/ChainArgos/java-monorepo/pulls/2110");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request.body)
            .map_err(|error| error.to_string())?,
        serde_json::json!({"base": "sea/integration"})
    );
    Ok(())
}

#[test]
fn retarget_mismatch_and_uncertain_response_never_repeat_patch() {
    let changed_head = pull_json("main", OLD_BASE, NEW_BASE);
    let responses = [
        (
            200,
            repository(BASE_REPOSITORY_ID, OWNER, BASE_REPOSITORY, true),
        ),
        (200, changed_head),
    ];
    let mut script = Script::responses(&responses);
    assert_eq!(
        update_pull_request_base(&mut script, base_target(), base_request(), "token"),
        Err(WorkflowRunError::RepositoryMismatch)
    );
    assert_eq!(script.seen.len(), 2);

    let before = [
        (
            200,
            repository(BASE_REPOSITORY_ID, OWNER, BASE_REPOSITORY, true),
        ),
        (200, pull_json("main", OLD_BASE, OLD_HEAD)),
    ];
    let mut script = Script::with_failure(&before, TransportFail::Timeout);
    assert_eq!(
        update_pull_request_base(&mut script, base_target(), base_request(), "token"),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Patch);
}

#[test]
fn malformed_retarget_response_stays_uncertain_after_one_patch() {
    let before = [
        (
            200,
            repository(BASE_REPOSITORY_ID, OWNER, BASE_REPOSITORY, true),
        ),
        (200, pull_json("main", OLD_BASE, OLD_HEAD)),
    ];
    let mut script = Script::responses(&before);
    script.replies.push_back(Ok(Exchange {
        status: 200,
        body: b"not json".to_vec(),
    }));
    assert_eq!(
        update_pull_request_base(&mut script, base_target(), base_request(), "token"),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Patch);
}

#[test]
fn nonce_commit_requires_exact_head_repository_and_uses_single_post() -> Result<(), String> {
    let mut script = Script::responses(&commit_responses());
    let receipt = create_pull_request_nonce_commit(
        &mut script,
        head_target(),
        "feature/review",
        OLD_HEAD,
        NONCE,
        &identity()?,
        "token",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(receipt.commit.sha, NONCE_COMMIT);
    assert_eq!(receipt.head_repository_id, HEAD_REPOSITORY_ID);
    assert_eq!(script.seen.len(), 4);
    let request = script.seen.get(2).ok_or("commit POST")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.path, "/repos/fork-owner/fork-repo/git/commits");
    let body: serde_json::Value =
        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
    assert_eq!(body["parents"], serde_json::json!([OLD_HEAD]));
    assert_eq!(body["tree"], TREE);
    assert_eq!(body["author"]["name"], AUTHOR_NAME);
    assert_eq!(body["author"]["email"], AUTHOR_EMAIL);
    assert_eq!(body["committer"]["name"], AUTHOR_NAME);
    assert_eq!(body["committer"]["email"], AUTHOR_EMAIL);
    assert_eq!(body["message"], COMMIT_MESSAGE_WITH_SIGNOFF);
    Ok(())
}

#[test]
fn nonce_commit_timeout_is_uncertain_and_is_not_retried() -> Result<(), String> {
    let mut before = commit_responses();
    before.truncate(2);
    let mut script = Script::with_failure(&before, TransportFail::Timeout);
    assert_eq!(
        create_pull_request_nonce_commit(
            &mut script,
            head_target(),
            "feature/review",
            OLD_HEAD,
            NONCE,
            &identity()?,
            "token"
        ),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Post);
    Ok(())
}

#[test]
fn malformed_nonce_commit_response_is_uncertain_after_one_post() -> Result<(), String> {
    let before = [
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (200, commit_json(OLD_HEAD, "Prior head", TREE, &[])),
    ];
    let mut script = Script::responses(&before);
    script.replies.push_back(Ok(Exchange {
        status: 201,
        body: b"not json".to_vec(),
    }));
    assert_eq!(
        create_pull_request_nonce_commit(
            &mut script,
            head_target(),
            "feature/review",
            OLD_HEAD,
            NONCE,
            &identity()?,
            "token"
        ),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Post);
    Ok(())
}

#[path = "workflow_run/publication.rs"]
mod publication;
