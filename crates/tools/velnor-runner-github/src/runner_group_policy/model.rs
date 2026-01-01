use std::collections::HashSet;

use serde::Deserialize;

use crate::{SessionError, WireError};

pub(super) const MAX_BODY_BYTES: usize = 1_048_576;

/// REST scope from which a group policy was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerGroupScope {
    /// Organization-owned runner group.
    Organization(String),
    /// Enterprise-owned runner group.
    Enterprise(String),
}

/// A repository that can access an organization runner group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedRepository {
    /// Immutable GitHub repository ID.
    pub id: i64,
    /// Repository name.
    pub name: String,
    /// Canonical `owner/repository` name.
    pub full_name: String,
    /// Privacy value, if the endpoint returned it.
    pub private: Option<bool>,
}

/// An organization that can access an enterprise runner group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedOrganization {
    /// Immutable GitHub organization ID.
    pub id: i64,
    /// Organization login.
    pub login: String,
}

/// The access mode and complete selected-scope inventory, when applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerGroupAccess {
    /// The group is visible to every repository or organization at its scope.
    All,
    /// The group is visible to private repositories only.
    Private,
    /// Complete repository list for an organization group with selected access.
    SelectedRepositories(Vec<SelectedRepository>),
    /// Complete organization list for an enterprise group with selected access.
    SelectedOrganizations(Vec<SelectedOrganization>),
}

/// Allowlisted REST policy metadata for one exact runner group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsRunnerGroupPolicy {
    /// REST scope used to address the group.
    pub scope: RunnerGroupScope,
    /// Positive group ID from the response.
    pub id: i64,
    /// Exact group name from the response.
    pub name: String,
    /// REST `visibility` value (`all`, `private`, or `selected`).
    pub visibility: String,
    /// Whether GitHub marks this as the default group, if returned.
    pub is_default: Option<bool>,
    /// Whether this group is inherited from an enterprise, if returned.
    pub inherited: Option<bool>,
    /// Whether public repositories can use the group, if returned.
    pub allows_public_repositories: Option<bool>,
    /// Whether workflow restrictions are enabled, if returned.
    pub restricted_to_workflows: Option<bool>,
    /// Exact workflow identities selected by GitHub, if returned.
    pub selected_workflows: Option<Vec<String>>,
    /// Whether workflow restrictions are read-only, if returned.
    pub workflow_restrictions_read_only: Option<bool>,
    /// Scope-level access mode and complete selected inventory.
    pub access: RunnerGroupAccess,
}

/// Group policy returned after a complete REST group inventory found one
/// unambiguous case-insensitive name match and its detail read agreed.
/// This is consistency evidence only; it does not prove that this REST group
/// controls a Scale Set in the Actions service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerGroupPolicySnapshot {
    /// Policy for the uniquely matched named group.
    pub policy: ActionsRunnerGroupPolicy,
    /// Number of groups in the fully reconciled REST inventory.
    pub inventory_group_count: usize,
}

pub(super) fn decode_group(body: &[u8], expected_id: i64) -> Result<GroupResponse, SessionError> {
    let group: GroupResponse = decode_body(body)?;
    if expected_id <= 0 || group.id != expected_id || !valid_group(&group) {
        return Err(WireError::Malformed.into());
    }
    Ok(group)
}

pub(super) fn decode_group_list(body: &[u8]) -> Result<CountPage<GroupResponse>, SessionError> {
    let page: GroupListPage = decode_body(body)?;
    if page.runner_groups.iter().any(|group| !valid_group(group)) {
        return Err(WireError::Malformed.into());
    }
    Ok(CountPage {
        total_count: page.total_count,
        items: page.runner_groups,
    })
}

pub(super) fn decode_repositories(body: &[u8]) -> Result<CountPage<GroupRepository>, SessionError> {
    let page: RepositoryPage = decode_body(body)?;
    Ok(CountPage {
        total_count: page.total_count,
        items: page
            .repositories
            .into_iter()
            .map(|repository| GroupRepository {
                id: repository.id,
                name: repository.name,
                full_name: repository.full_name,
                private: repository.private,
            })
            .collect(),
    })
}

pub(super) fn decode_organizations(
    body: &[u8],
) -> Result<CountPage<GroupOrganization>, SessionError> {
    let page: OrganizationPage = decode_body(body)?;
    Ok(CountPage {
        total_count: page.total_count,
        items: page
            .organizations
            .into_iter()
            .map(|organization| GroupOrganization {
                id: organization.id,
                login: organization.login,
            })
            .collect(),
    })
}

pub(super) fn decode_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, SessionError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(WireError::Malformed.into());
    }
    serde_json::from_slice(body).map_err(|_| WireError::Malformed.into())
}

pub(super) fn path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(super) fn valid_group_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && valid_text(value)
}

fn valid_group(group: &GroupResponse) -> bool {
    group.id > 0
        && valid_group_name(&group.name)
        && matches!(group.visibility.as_str(), "all" | "private" | "selected")
        && group.selected_workflows.as_ref().is_none_or(|workflows| {
            let mut seen = HashSet::new();
            workflows
                .iter()
                .all(|workflow| valid_text(workflow) && seen.insert(workflow))
        })
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_control())
}

pub(super) trait PageEntry {
    fn id(&self) -> i64;
    fn key(&self) -> &str;
    fn is_valid(&self) -> bool;
}

impl PageEntry for GroupRepository {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> &str {
        &self.full_name
    }

    fn is_valid(&self) -> bool {
        let mut names = self.full_name.split('/');
        self.id > 0
            && path_segment(&self.name)
            && names.next().is_some_and(path_segment)
            && names.next().is_some_and(path_segment)
            && names.next().is_none()
    }
}

impl PageEntry for GroupOrganization {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> &str {
        &self.login
    }

    fn is_valid(&self) -> bool {
        self.id > 0 && path_segment(&self.login)
    }
}

impl PageEntry for GroupResponse {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> &str {
        &self.name
    }

    fn is_valid(&self) -> bool {
        valid_group(self)
    }
}

pub(super) struct CountPage<T> {
    pub(super) total_count: i64,
    pub(super) items: Vec<T>,
}

#[derive(Deserialize)]
struct GroupListPage {
    total_count: i64,
    runner_groups: Vec<GroupResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct GroupResponse {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) visibility: String,
    #[serde(default, rename = "default")]
    pub(super) is_default: Option<bool>,
    #[serde(default)]
    pub(super) inherited: Option<bool>,
    #[serde(default)]
    pub(super) allows_public_repositories: Option<bool>,
    #[serde(default)]
    pub(super) restricted_to_workflows: Option<bool>,
    #[serde(default)]
    pub(super) selected_workflows: Option<Vec<String>>,
    #[serde(default)]
    pub(super) workflow_restrictions_read_only: Option<bool>,
}

#[derive(Deserialize)]
struct RepositoryPage {
    total_count: i64,
    repositories: Vec<RepositoryItem>,
}

#[derive(Deserialize)]
struct RepositoryItem {
    id: i64,
    name: String,
    full_name: String,
    #[serde(default)]
    private: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroupRepository {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) private: Option<bool>,
}

#[derive(Deserialize)]
struct OrganizationPage {
    total_count: i64,
    organizations: Vec<OrganizationItem>,
}

#[derive(Deserialize)]
struct OrganizationItem {
    id: i64,
    login: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroupOrganization {
    pub(super) id: i64,
    pub(super) login: String,
}
