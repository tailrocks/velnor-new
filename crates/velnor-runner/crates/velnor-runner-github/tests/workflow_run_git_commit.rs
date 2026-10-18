//! Exact Git commit lookup checks.

use std::collections::{HashMap, VecDeque};

use velnor_runner_github::workflow_run::{
    GITHUB_API_VERSION, WorkflowRunError, get_git_commit, is_commit_ancestor,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const OWNER: &str = "ChainArgos";
const REPOSITORY: &str = "java-monorepo";
const COMMIT_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TREE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PARENT_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn response(status: u16, body: &str) -> Self {
        Self {
            replies: VecDeque::from([Ok(Exchange {
                status,
                body: body.as_bytes().to_vec(),
            })]),
            seen: Vec::new(),
        }
    }

    fn failure(failure: TransportFail) -> Self {
        Self {
            replies: VecDeque::from([Err(failure)]),
            seen: Vec::new(),
        }
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

fn commit_json(sha: &str, tree: &str, parents: &[&str]) -> String {
    let parents = parents
        .iter()
        .map(|parent| format!(r#"{{"sha":"{parent}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"sha":"{sha}","message":"nonce source","tree":{{"sha":"{tree}"}},"parents":[{parents}]}}"#
    )
}

#[test]
fn reads_exact_commit_from_authenticated_repository() -> Result<(), String> {
    let mut script = Script::response(200, &commit_json(COMMIT_SHA, TREE_SHA, &[PARENT_SHA]));
    let commit = get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token-canary")
        .map_err(|error| error.to_string())?;
    assert_eq!(commit.sha, COMMIT_SHA);
    assert_eq!(commit.tree.sha, TREE_SHA);
    assert_eq!(commit.parents.len(), 1);
    assert_eq!(commit.parents[0].sha, PARENT_SHA);
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.path,
        format!("/repos/{OWNER}/{REPOSITORY}/git/commits/{COMMIT_SHA}")
    );
    assert_eq!(request.query, None);
    assert_eq!(request.body.len(), 0);
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
    Ok(())
}

#[test]
fn rejects_malformed_sha_before_transport_and_response_identity_errors() {
    let mut script = Script::response(200, &commit_json(COMMIT_SHA, TREE_SHA, &[]));
    assert_eq!(
        get_git_commit(
            &mut script,
            OWNER,
            REPOSITORY,
            "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "token"
        ),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert_eq!(script.seen.len(), 0);

    let wrong_sha = commit_json(PARENT_SHA, TREE_SHA, &[]);
    let mut script = Script::response(200, &wrong_sha);
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let wrong_tree = commit_json(COMMIT_SHA, "not-a-sha", &[]);
    let mut script = Script::response(200, &wrong_tree);
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let wrong_parent = commit_json(COMMIT_SHA, TREE_SHA, &["not-a-sha"]);
    let mut script = Script::response(200, &wrong_parent);
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::Malformed)
    );
}

#[test]
fn preserves_uncertain_and_completed_http_failures() {
    let mut script = Script::failure(TransportFail::Timeout);
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::Uncertain)
    );

    let mut script = Script::response(404, "{}");
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::HttpStatus(404))
    );

    let mut script = Script::response(503, "{}");
    assert_eq!(
        get_git_commit(&mut script, OWNER, REPOSITORY, COMMIT_SHA, "token"),
        Err(WorkflowRunError::Uncertain)
    );
}

struct CommitGraph {
    commits: HashMap<String, String>,
    seen: Vec<SessionRequest>,
}

impl Transport for CommitGraph {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        let sha = request
            .path
            .rsplit('/')
            .next()
            .ok_or(TransportFail::Reset)?;
        let Some(body) = self.commits.get(sha) else {
            return Ok(Exchange {
                status: 404,
                body: b"{}".to_vec(),
            });
        };
        Ok(Exchange {
            status: 200,
            body: body.as_bytes().to_vec(),
        })
    }
}

#[test]
fn proves_commit_ancestry_across_merge_graph_and_rejects_unrelated_commits() -> Result<(), String> {
    let root = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let left = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let right = "cccccccccccccccccccccccccccccccccccccccc";
    let merge = "dddddddddddddddddddddddddddddddddddddddd";
    let unrelated = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let mut graph = CommitGraph {
        commits: HashMap::from([
            (
                merge.to_owned(),
                commit_json(merge, TREE_SHA, &[left, right]),
            ),
            (left.to_owned(), commit_json(left, TREE_SHA, &[root])),
            (right.to_owned(), commit_json(right, TREE_SHA, &[root])),
            (unrelated.to_owned(), commit_json(unrelated, TREE_SHA, &[])),
        ]),
        seen: Vec::new(),
    };

    assert!(
        is_commit_ancestor(&mut graph, OWNER, REPOSITORY, root, merge, "token")
            .map_err(|error| error.to_string())?
    );
    let requests_before_unrelated = graph.seen.len();
    assert!(
        !is_commit_ancestor(&mut graph, OWNER, REPOSITORY, root, unrelated, "token")
            .map_err(|error| error.to_string())?
    );
    assert_eq!(graph.seen.len(), requests_before_unrelated + 1);
    Ok(())
}

#[test]
fn ancestry_identity_and_missing_objects_fail_closed() {
    let root = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let missing = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut graph = CommitGraph {
        commits: HashMap::new(),
        seen: Vec::new(),
    };
    assert_eq!(
        is_commit_ancestor(&mut graph, OWNER, REPOSITORY, root, root, "token"),
        Ok(true)
    );
    assert_eq!(graph.seen.len(), 0);
    assert_eq!(
        is_commit_ancestor(&mut graph, OWNER, REPOSITORY, root, missing, "token"),
        Err(WorkflowRunError::HttpStatus(404))
    );
    assert_eq!(graph.seen.len(), 1);
}
