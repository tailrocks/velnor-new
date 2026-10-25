//! Immutable workflow content lookup checks.

use std::collections::VecDeque;

use velnor_runner_github::workflow_run::{
    RepositoryTarget, WorkflowRunError, get_workflow_content,
};
use velnor_runner_github::{Exchange, SessionRequest, Transport, TransportFail};

pub(super) const OWNER: &str = "ChainArgos";
pub(super) const REPOSITORY: &str = "java-monorepo";
pub(super) const REPOSITORY_ID: i64 = 41;
pub(super) const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const ROOT_TREE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const GITHUB_TREE: &str = "cccccccccccccccccccccccccccccccccccccccc";
pub(super) const WORKFLOWS_TREE: &str = "dddddddddddddddddddddddddddddddddddddddd";
pub(super) const BLOB: &str = "c87443c43f5b9bcc3b4087b87d53f1785ee3a05e";
pub(super) const CONTENT: &str = "name: ci\n";
pub(super) const CONTENT_SHA256: &str =
    "559e3b9f2cee0c0809bf40661fff3e2fed2644e1d0e07ce2204ed2ba39ea11b3";

pub(super) struct Script {
    pub(super) replies: VecDeque<Result<Exchange, TransportFail>>,
    pub(super) requests: Vec<SessionRequest>,
}

impl Script {
    pub(super) fn valid() -> Self {
        Self::new(vec![
            response(repository_json()),
            response(commit_json(ROOT_TREE)),
            response(tree_json(
                ROOT_TREE,
                &[entry(".github", "040000", "tree", GITHUB_TREE, None)],
            )),
            response(tree_json(
                GITHUB_TREE,
                &[entry("workflows", "040000", "tree", WORKFLOWS_TREE, None)],
            )),
            response(tree_json(
                WORKFLOWS_TREE,
                &[entry("ci.yml", "100644", "blob", BLOB, Some(CONTENT.len()))],
            )),
            response(blob_json(BLOB, "base64", CONTENT.len(), "bmFtZTogY2kK")),
        ])
    }

    pub(super) fn new(replies: Vec<Exchange>) -> Self {
        Self {
            replies: replies.into_iter().map(Ok).collect(),
            requests: Vec::new(),
        }
    }

    pub(super) fn failure(failure: TransportFail) -> Self {
        Self {
            replies: VecDeque::from([Err(failure)]),
            requests: Vec::new(),
        }
    }

    pub(super) fn replace(&mut self, index: usize, body: String) {
        self.replies[index] = Ok(response(body));
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(request.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

pub(super) fn response(body: String) -> Exchange {
    response_status(200, body)
}

pub(super) fn response_status(status: u16, body: String) -> Exchange {
    Exchange {
        status,
        body: body.into_bytes(),
    }
}

pub(super) fn target() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: REPOSITORY_ID,
        owner: OWNER,
        name: REPOSITORY,
    }
}

pub(super) fn repository_json() -> String {
    format!(
        r#"{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","private":true,"owner":{{"login":"{OWNER}"}}}}"#
    )
}

pub(super) fn commit_json(tree: &str) -> String {
    format!(r#"{{"sha":"{COMMIT}","message":"source","tree":{{"sha":"{tree}"}},"parents":[]}}"#)
}

pub(super) fn commit_json_with_sha(sha: &str, tree: &str) -> String {
    format!(r#"{{"sha":"{sha}","message":"source","tree":{{"sha":"{tree}"}},"parents":[]}}"#)
}

pub(super) fn entry(path: &str, mode: &str, kind: &str, sha: &str, size: Option<usize>) -> String {
    let size = size.map_or_else(String::new, |size| format!(r#", "size":{size}"#));
    format!(r#"{{"path":"{path}","mode":"{mode}","type":"{kind}","sha":"{sha}"{size}}}"#)
}

pub(super) fn tree_json(sha: &str, entries: &[String]) -> String {
    format!(
        r#"{{"sha":"{sha}","tree":[{}],"truncated":false}}"#,
        entries.join(",")
    )
}

pub(super) fn blob_json(sha: &str, encoding: &str, size: usize, content: &str) -> String {
    let content = content.replace('\n', "\\n").replace('\r', "\\r");
    format!(r#"{{"sha":"{sha}","encoding":"{encoding}","size":{size},"content":"{content}"}}"#)
}

pub(super) fn read(script: &mut Script, path: &str) -> Result<(), WorkflowRunError> {
    get_workflow_content(script, target(), COMMIT, path, "token-canary").map(|_| ())
}
