use crate::actions::{actions_request, status_error};
use crate::registration::{AsyncDiscoveryTransport, execute_discovery};
use crate::session::execute;
use crate::{SessionError, Transport, WireError};

use super::model::{
    ActionsRunnerGroupPolicy, MAX_BODY_BYTES, RunnerGroupAccess, RunnerGroupScope,
    SelectedOrganization, SelectedRepository, decode_group, decode_organizations,
    decode_repositories, path_segment,
};
use super::pages::read_pages;

/// Read one organization group's REST metadata and complete selected repository list.
///
/// The transport must already be bound to the intended GitHub REST origin and
/// enforce whole-request deadlines and streaming response limits. This helper
/// uses relative fixed paths, performs GET only, and never follows server-provided
/// pagination URLs. A selected list exceeding 3,200 entries fails closed.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid input,
/// [`WireError::Malformed`] for incomplete or inconsistent responses, and the
/// existing status/transport error classes for failed requests. A 404 is not
/// interpreted as permission proof or as a reason to create a group.
pub fn get_organization_runner_group_policy<T>(
    transport: &mut T,
    organization: &str,
    group_id: i64,
    actions_token: &str,
) -> Result<ActionsRunnerGroupPolicy, SessionError>
where
    T: Transport + ?Sized,
{
    read_group_policy(
        transport,
        RunnerGroupScope::Organization(organization.to_owned()),
        group_id,
        actions_token,
    )
}

/// Read one enterprise group's REST metadata and complete selected organization list.
///
/// The transport must already be bound to the intended GitHub REST origin and
/// enforce whole-request deadlines and streaming response limits. This helper
/// uses relative fixed paths, performs GET only, and never follows server-provided
/// pagination URLs. A selected list exceeding 3,200 entries fails closed.
/// Enterprise group access does not by itself prove repository-level access.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid input,
/// [`WireError::Malformed`] for incomplete or inconsistent responses, and the
/// existing status/transport error classes for failed requests. A 404 is not
/// interpreted as permission proof or as a reason to create a group.
pub fn get_enterprise_runner_group_policy<T>(
    transport: &mut T,
    enterprise: &str,
    group_id: i64,
    actions_token: &str,
) -> Result<ActionsRunnerGroupPolicy, SessionError>
where
    T: Transport + ?Sized,
{
    read_group_policy(
        transport,
        RunnerGroupScope::Enterprise(enterprise.to_owned()),
        group_id,
        actions_token,
    )
}

pub(super) fn read_group_policy<T>(
    transport: &mut T,
    scope: RunnerGroupScope,
    group_id: i64,
    actions_token: &str,
) -> Result<ActionsRunnerGroupPolicy, SessionError>
where
    T: Transport + ?Sized,
{
    let group_path = group_path(&scope, group_id, "")?;
    let response = get(transport, &group_path, None, actions_token)?;
    if response.status != 200 {
        return Err(status_error(response.status));
    }
    let group = decode_group(&response.body, group_id)?;
    let access = read_access(
        transport,
        &scope,
        group_id,
        &group.visibility,
        actions_token,
    )?;
    Ok(ActionsRunnerGroupPolicy {
        scope,
        id: group.id,
        name: group.name,
        visibility: group.visibility,
        is_default: group.is_default,
        inherited: group.inherited,
        allows_public_repositories: group.allows_public_repositories,
        restricted_to_workflows: group.restricted_to_workflows,
        selected_workflows: group.selected_workflows,
        workflow_restrictions_read_only: group.workflow_restrictions_read_only,
        access,
    })
}

pub(super) async fn read_group_policy_async<T>(
    transport: &mut T,
    scope: RunnerGroupScope,
    group_id: i64,
    actions_token: &str,
) -> Result<ActionsRunnerGroupPolicy, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let path = group_path(&scope, group_id, "")?;
    let response = get_async(transport, &path, None, actions_token).await?;
    if response.status != 200 {
        return Err(status_error(response.status));
    }
    let group = decode_group(&response.body, group_id)?;
    let access = read_access_async(
        transport,
        &scope,
        group_id,
        &group.visibility,
        actions_token,
    )
    .await?;
    Ok(ActionsRunnerGroupPolicy {
        scope,
        id: group.id,
        name: group.name,
        visibility: group.visibility,
        is_default: group.is_default,
        inherited: group.inherited,
        allows_public_repositories: group.allows_public_repositories,
        restricted_to_workflows: group.restricted_to_workflows,
        selected_workflows: group.selected_workflows,
        workflow_restrictions_read_only: group.workflow_restrictions_read_only,
        access,
    })
}

fn read_access<T>(
    transport: &mut T,
    scope: &RunnerGroupScope,
    group_id: i64,
    visibility: &str,
    actions_token: &str,
) -> Result<RunnerGroupAccess, SessionError>
where
    T: Transport + ?Sized,
{
    match visibility {
        "all" => Ok(RunnerGroupAccess::All),
        "private" => Ok(RunnerGroupAccess::Private),
        "selected" => read_selected_access(transport, scope, group_id, actions_token),
        _ => Err(WireError::Malformed.into()),
    }
}

async fn read_access_async<T>(
    transport: &mut T,
    scope: &RunnerGroupScope,
    group_id: i64,
    visibility: &str,
    actions_token: &str,
) -> Result<RunnerGroupAccess, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    match visibility {
        "all" => Ok(RunnerGroupAccess::All),
        "private" => Ok(RunnerGroupAccess::Private),
        "selected" => read_selected_access_async(transport, scope, group_id, actions_token).await,
        _ => Err(WireError::Malformed.into()),
    }
}

fn read_selected_access<T>(
    transport: &mut T,
    scope: &RunnerGroupScope,
    group_id: i64,
    actions_token: &str,
) -> Result<RunnerGroupAccess, SessionError>
where
    T: Transport + ?Sized,
{
    match scope {
        RunnerGroupScope::Organization(_organization) => {
            let path = group_path(scope, group_id, "repositories")?;
            let repositories = read_pages(transport, &path, actions_token, decode_repositories)?;
            Ok(RunnerGroupAccess::SelectedRepositories(
                repositories
                    .into_iter()
                    .map(|repository| SelectedRepository {
                        id: repository.id,
                        name: repository.name,
                        full_name: repository.full_name,
                        private: repository.private,
                    })
                    .collect(),
            ))
        }
        RunnerGroupScope::Enterprise(_) => {
            let path = group_path(scope, group_id, "organizations")?;
            let organizations = read_pages(transport, &path, actions_token, decode_organizations)?;
            Ok(RunnerGroupAccess::SelectedOrganizations(
                organizations
                    .into_iter()
                    .map(|organization| SelectedOrganization {
                        id: organization.id,
                        login: organization.login,
                    })
                    .collect(),
            ))
        }
    }
}

async fn read_selected_access_async<T>(
    transport: &mut T,
    scope: &RunnerGroupScope,
    group_id: i64,
    actions_token: &str,
) -> Result<RunnerGroupAccess, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    match scope {
        RunnerGroupScope::Organization(_) => {
            let path = group_path(scope, group_id, "repositories")?;
            let repositories = super::pages::read_pages_async(
                transport,
                &path,
                actions_token,
                decode_repositories,
            )
            .await?;
            Ok(RunnerGroupAccess::SelectedRepositories(
                repositories
                    .into_iter()
                    .map(|repository| SelectedRepository {
                        id: repository.id,
                        name: repository.name,
                        full_name: repository.full_name,
                        private: repository.private,
                    })
                    .collect(),
            ))
        }
        RunnerGroupScope::Enterprise(_) => {
            let path = group_path(scope, group_id, "organizations")?;
            let organizations = super::pages::read_pages_async(
                transport,
                &path,
                actions_token,
                decode_organizations,
            )
            .await?;
            Ok(RunnerGroupAccess::SelectedOrganizations(
                organizations
                    .into_iter()
                    .map(|organization| SelectedOrganization {
                        id: organization.id,
                        login: organization.login,
                    })
                    .collect(),
            ))
        }
    }
}

pub(super) fn group_path(
    scope: &RunnerGroupScope,
    group_id: i64,
    suffix: &str,
) -> Result<String, SessionError> {
    if group_id <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    let (prefix, slug) = match scope {
        RunnerGroupScope::Organization(value) => ("orgs", value),
        RunnerGroupScope::Enterprise(value) => ("enterprises", value),
    };
    if !path_segment(slug) {
        return Err(WireError::RegistrationRejected.into());
    }
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!("/{suffix}")
    };
    Ok(format!(
        "{prefix}/{slug}/actions/runner-groups/{group_id}{suffix}"
    ))
}

pub(super) fn group_collection_path(scope: &RunnerGroupScope) -> Result<String, SessionError> {
    let (prefix, slug) = match scope {
        RunnerGroupScope::Organization(value) => ("orgs", value),
        RunnerGroupScope::Enterprise(value) => ("enterprises", value),
    };
    if !path_segment(slug) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(format!("{prefix}/{slug}/actions/runner-groups"))
}

pub(super) async fn get_async<T>(
    transport: &mut T,
    path: &str,
    query: Option<String>,
    actions_token: &str,
) -> Result<crate::Exchange, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let mut request = actions_request(path.to_owned(), actions_token)?;
    request.query = query;
    let response = execute_discovery(transport, request).await?;
    if response.body.len() > MAX_BODY_BYTES {
        return Err(WireError::Malformed.into());
    }
    Ok(response)
}

pub(super) fn get<T>(
    transport: &mut T,
    path: &str,
    query: Option<String>,
    actions_token: &str,
) -> Result<crate::Exchange, SessionError>
where
    T: Transport + ?Sized,
{
    let mut request = actions_request(path.to_owned(), actions_token)?;
    request.query = query;
    let response = execute(transport, &request)?;
    if response.body.len() > MAX_BODY_BYTES {
        return Err(WireError::Malformed.into());
    }
    Ok(response)
}
