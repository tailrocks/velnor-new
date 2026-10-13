use crate::{SessionError, Transport, WireError};

use super::model::{GroupResponse, RunnerGroupPolicySnapshot, RunnerGroupScope, decode_group_list};
use super::pages::read_pages;
use super::reader::{group_collection_path, read_group_policy};

/// Find one exact group name in the complete REST inventory for an organization.
///
/// The returned snapshot is consistency evidence only. It does not prove that
/// this REST group controls an Actions Service Scale Set. A missing exact name
/// returns `Ok(None)`; incomplete pages, duplicate identities, or metadata that
/// changes between list and detail reads fail closed.
///
/// The transport must be bound to the intended GitHub REST origin and enforce
/// whole-request deadlines and streaming response limits. Requests are GET-only
/// and use fixed relative paths.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid scope or token,
/// [`WireError::Malformed`] for incomplete or inconsistent inventory, and the
/// existing status/transport error classes for failed requests.
pub fn find_organization_runner_group_policy<T>(
    transport: &mut T,
    organization: &str,
    exact_name: &str,
    actions_token: &str,
) -> Result<Option<RunnerGroupPolicySnapshot>, SessionError>
where
    T: Transport + ?Sized,
{
    find_runner_group_policy(
        transport,
        RunnerGroupScope::Organization(organization.to_owned()),
        exact_name,
        actions_token,
    )
}

/// Find one exact group name in the complete REST inventory for an enterprise.
///
/// Enterprise group access does not by itself prove repository-level access.
/// The returned snapshot is consistency evidence only and does not prove that
/// this REST group controls an Actions Service Scale Set.
///
/// The transport must be bound to the intended GitHub REST origin and enforce
/// whole-request deadlines and streaming response limits. Requests are GET-only
/// and use fixed relative paths.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid scope or token,
/// [`WireError::Malformed`] for incomplete or inconsistent inventory, and the
/// existing status/transport error classes for failed requests.
pub fn find_enterprise_runner_group_policy<T>(
    transport: &mut T,
    enterprise: &str,
    exact_name: &str,
    actions_token: &str,
) -> Result<Option<RunnerGroupPolicySnapshot>, SessionError>
where
    T: Transport + ?Sized,
{
    find_runner_group_policy(
        transport,
        RunnerGroupScope::Enterprise(enterprise.to_owned()),
        exact_name,
        actions_token,
    )
}

fn find_runner_group_policy<T>(
    transport: &mut T,
    scope: RunnerGroupScope,
    exact_name: &str,
    actions_token: &str,
) -> Result<Option<RunnerGroupPolicySnapshot>, SessionError>
where
    T: Transport + ?Sized,
{
    if exact_name.is_empty()
        || exact_name.len() > 256
        || exact_name.bytes().any(|byte| byte.is_ascii_control())
        || actions_token.is_empty()
    {
        return Err(WireError::RegistrationRejected.into());
    }

    let path = group_collection_path(&scope)?;
    let groups = read_pages(transport, &path, actions_token, decode_group_list)?;
    let inventory_group_count = groups.len();
    let mut matches = groups.into_iter().filter(|group| group.name == exact_name);
    let Some(listed) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(WireError::Malformed.into());
    }

    let policy = read_group_policy(transport, scope, listed.id, actions_token)?;
    if !listed_detail_agree(&listed, &policy) {
        return Err(WireError::Malformed.into());
    }
    Ok(Some(RunnerGroupPolicySnapshot {
        policy,
        inventory_group_count,
    }))
}

fn listed_detail_agree(
    listed: &GroupResponse,
    detail: &super::model::ActionsRunnerGroupPolicy,
) -> bool {
    listed.id == detail.id
        && listed.name == detail.name
        && listed.visibility == detail.visibility
        && optional_agrees(listed.is_default.as_ref(), detail.is_default.as_ref())
        && optional_agrees(listed.inherited.as_ref(), detail.inherited.as_ref())
        && optional_agrees(
            listed.allows_public_repositories.as_ref(),
            detail.allows_public_repositories.as_ref(),
        )
        && optional_agrees(
            listed.restricted_to_workflows.as_ref(),
            detail.restricted_to_workflows.as_ref(),
        )
        && optional_agrees(
            listed.selected_workflows.as_ref(),
            detail.selected_workflows.as_ref(),
        )
        && optional_agrees(
            listed.workflow_restrictions_read_only.as_ref(),
            detail.workflow_restrictions_read_only.as_ref(),
        )
}

fn optional_agrees<T: PartialEq>(listed: Option<&T>, detail: Option<&T>) -> bool {
    listed.is_none_or(|listed| Some(listed) == detail)
}
