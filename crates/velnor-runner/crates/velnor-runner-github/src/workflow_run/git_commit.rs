// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashSet, VecDeque};

use serde::Deserialize;

use super::{WorkflowRunError, repository_path};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

const MAX_ANCESTRY_COMMITS: usize = 4096;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitCommitParent {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitCommitTree {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitCommitPerson {
    pub name: String,

    pub email: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitCommit {
    pub sha: String,

    pub message: String,

    #[serde(default)]
    pub author: Option<GitCommitPerson>,

    #[serde(default)]
    pub committer: Option<GitCommitPerson>,

    pub tree: GitCommitTree,

    pub parents: Vec<GitCommitParent>,
}

pub fn get_git_commit<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    sha: &str,
    token: &str,
) -> Result<GitCommit, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if !valid_sha(sha) {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let path = repository_path(owner, repository, &format!("git/commits/{sha}"))?;
    let request = SessionRequest {
        method: Method::Get,
        path,
        query: None,
        headers: super::api_headers(token)?,
        body: Vec::new(),
    };
    let exchange = match transport.exchange(&request) {
        Ok(exchange) => exchange,
        Err(TransportFail::Timeout | TransportFail::Reset) => {
            return Err(WorkflowRunError::Uncertain);
        }
        Err(TransportFail::Http(status)) => return Err(super::api_status(status)),
    };
    decode_commit(&exchange, sha)
}

pub fn is_commit_ancestor<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    ancestor_sha: &str,
    descendant_sha: &str,
    token: &str,
) -> Result<bool, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if !valid_sha(ancestor_sha) || !valid_sha(descendant_sha) {
        return Err(WorkflowRunError::InvalidRequest);
    }
    if ancestor_sha == descendant_sha {
        return Ok(true);
    }

    let mut pending = VecDeque::from([descendant_sha.to_owned()]);
    let mut discovered = HashSet::from([descendant_sha.to_owned()]);
    while let Some(sha) = pending.pop_front() {
        let commit = get_git_commit(transport, owner, repository, &sha, token)?;
        for parent in commit.parents {
            if parent.sha == ancestor_sha {
                return Ok(true);
            }
            if discovered.insert(parent.sha.clone()) {
                if discovered.len() > MAX_ANCESTRY_COMMITS {
                    return Err(WorkflowRunError::ResponseTooLarge);
                }
                pending.push_back(parent.sha);
            }
        }
    }
    Ok(false)
}

fn decode_commit(exchange: &Exchange, requested_sha: &str) -> Result<GitCommit, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    let commit: GitCommit =
        serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Malformed)?;
    if !valid_sha(&commit.sha)
        || commit.sha != requested_sha
        || !valid_sha(&commit.tree.sha)
        || commit.parents.iter().any(|parent| !valid_sha(&parent.sha))
    {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(commit)
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
