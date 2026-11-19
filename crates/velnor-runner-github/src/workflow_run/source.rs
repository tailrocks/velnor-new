// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryTarget<'a> {
    pub id: i64,

    pub owner: &'a str,

    pub name: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryScope {
    id: i64,
    owner: String,
    name: String,
}

impl VerifiedRepositoryScope {
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryIdentity {
    id: i64,
    owner: String,
    name: String,
    private: bool,
}

impl VerifiedRepositoryIdentity {
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn is_private(&self) -> bool {
        self.private
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepositoryScopeMismatch {
    Id,
    Name,
    Public,
}

pub(super) fn verify_repository_scope(
    repository: &super::GitHubRepository,
    expected: RepositoryTarget<'_>,
) -> Result<VerifiedRepositoryScope, RepositoryScopeMismatch> {
    if expected.id <= 0 || repository.id != expected.id {
        return Err(RepositoryScopeMismatch::Id);
    }
    let full_name = format!("{}/{}", expected.owner, expected.name);
    if expected.owner.is_empty()
        || expected.name.is_empty()
        || !same_name(&repository.owner.login, expected.owner)
        || !same_name(&repository.name, expected.name)
        || !same_name(&repository.full_name, &full_name)
    {
        return Err(RepositoryScopeMismatch::Name);
    }
    if !repository.private {
        return Err(RepositoryScopeMismatch::Public);
    }
    Ok(VerifiedRepositoryScope {
        id: repository.id,
        owner: repository.owner.login.clone(),
        name: repository.name.clone(),
    })
}

pub(super) fn verify_repository_identity(
    repository: &super::GitHubRepository,
    expected: RepositoryTarget<'_>,
) -> Result<VerifiedRepositoryIdentity, RepositoryScopeMismatch> {
    if expected.id <= 0 || repository.id != expected.id {
        return Err(RepositoryScopeMismatch::Id);
    }
    let full_name = format!("{}/{}", expected.owner, expected.name);
    if expected.owner.is_empty()
        || expected.name.is_empty()
        || !same_name(&repository.owner.login, expected.owner)
        || !same_name(&repository.name, expected.name)
        || !same_name(&repository.full_name, &full_name)
    {
        return Err(RepositoryScopeMismatch::Name);
    }
    Ok(VerifiedRepositoryIdentity {
        id: repository.id,
        owner: repository.owner.login.clone(),
        name: repository.name.clone(),
        private: repository.private,
    })
}

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

pub(super) fn valid_commit_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

mod pull_request;
mod workflow;

pub use pull_request::{
    PullRequestMismatch, VerifiedPullRequest, VerifiedPullRequestRunAssociation,
    verify_pull_request, verify_pull_request_run_association,
};
pub use workflow::{SourceMismatch, VerifiedWorkflowSource, verify_workflow_source};
