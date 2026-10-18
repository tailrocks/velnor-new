//! Source checks for authenticated job metadata.

/// Expected repository identity for one runner connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryTarget<'a> {
    /// Stable repository identifier.
    pub id: i64,
    /// GitHub account name.
    pub owner: &'a str,
    /// GitHub repository name.
    pub name: &'a str,
}

/// Repository identity returned only after an authenticated exact-scope lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryScope {
    id: i64,
    owner: String,
    name: String,
}

impl VerifiedRepositoryScope {
    /// Immutable repository ID from GitHub.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Canonical owner login from GitHub.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Canonical repository name from GitHub.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Repository identity returned only after an authenticated exact-scope lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryIdentity {
    id: i64,
    owner: String,
    name: String,
    private: bool,
}

impl VerifiedRepositoryIdentity {
    /// Immutable repository ID from GitHub.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Canonical owner login from GitHub.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Canonical repository name from GitHub.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether GitHub marks the repository private.
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
