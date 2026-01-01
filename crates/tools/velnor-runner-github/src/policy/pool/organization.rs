use std::time::SystemTime;

use crate::{
    ActionsRunnerGroupPolicy, ActionsServiceScaleSetRoute, OrganizationRunnerGroupPolicyEvidence,
    RunnerGroupAccess, RunnerGroupScope, ScaleSetView, accept_scale_set_for,
};

use super::super::types::{PolicyGap, PolicyMismatch, PoolBindingView, PoolRegistrationScopeView};
use super::repository::RepositoryPoolTrustEvidence;
use super::snapshot::{build_snapshot, effective_route_proof};
use super::{PoolAdmissionEvidence, verify_pool_policy_with_proof};

pub(super) struct GroupPolicyFacts<'a> {
    pub(super) policy: &'a ActionsRunnerGroupPolicy,
    pub(super) selected_repository_ids: Vec<i64>,
    pub(super) selected_workflows: &'a [String],
}

/// Join exact-scope REST group policy to its Actions Service Scale Set route and
/// issue an opaque pool-policy proof when all restrictions are explicit.
///
/// Repository-scoped Sets intentionally remain Unknown here. The organization
/// name is joined within that exact scope; REST and Actions Service numeric
/// group IDs remain distinct. This is pool-policy evidence only: the caller
/// still needs a state-owned fenced zero-assigned/running observation and
/// separate per-offer trust before Acquire/JIT.
#[must_use]
pub fn verify_organization_pool_policy(
    expected: &PoolBindingView<'_>,
    repository: &RepositoryPoolTrustEvidence,
    runner_group: &OrganizationRunnerGroupPolicyEvidence,
    route: &ActionsServiceScaleSetRoute,
    now: SystemTime,
) -> PoolAdmissionEvidence {
    let Some(organization) = organization_scope(expected.registration_scope) else {
        return PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven);
    };
    if expected.allowed_group_workflows.is_empty() {
        return PoolAdmissionEvidence::Unknown(PolicyGap::WorkflowRuleSetEmpty);
    }
    if !valid_expected(expected, organization) {
        return PoolAdmissionEvidence::Rejected(PolicyMismatch::InvalidPolicy);
    }
    if let Err(evidence) = validate_repository(expected, repository) {
        return evidence;
    }
    let scale_set = match validate_route(expected, organization, route) {
        Ok(scale_set) => scale_set,
        Err(evidence) => return evidence,
    };
    let facts = match validate_group(
        expected,
        organization,
        repository.repository_id,
        runner_group,
    ) {
        Ok(facts) => facts,
        Err(evidence) => return evidence,
    };

    let observed = build_snapshot(
        expected,
        organization,
        repository,
        runner_group,
        route,
        scale_set,
        &facts,
    );
    let proof = effective_route_proof(
        expected,
        organization,
        repository,
        runner_group,
        route,
        scale_set,
        now,
    );
    verify_pool_policy_with_proof(expected, &observed, Some(&proof), now)
}

pub(super) fn organization_scope(scope: PoolRegistrationScopeView<'_>) -> Option<&str> {
    match scope {
        PoolRegistrationScopeView::Organization { organization } => Some(organization),
        PoolRegistrationScopeView::Repository { .. } => None,
    }
}

pub(super) fn valid_expected(expected: &PoolBindingView<'_>, organization: &str) -> bool {
    let Some((repository_owner, repository_name)) =
        expected.target_repository_full_name.split_once('/')
    else {
        return false;
    };
    expected.target_repository_id.is_none_or(|id| id > 0)
        && expected.scale_set_id.is_none_or(|id| id > 0)
        && expected.actions_runner_group_id > 0
        && expected.rest_runner_group_id.is_none_or(|id| id > 0)
        && repository_owner.eq_ignore_ascii_case(organization)
        && !repository_name.is_empty()
        && !repository_name.contains('/')
        && !expected.scale_set_name.is_empty()
        && !expected.actions_runner_group_name.is_empty()
        && expected.runner_image.is_some_and(|image| {
            !image.profile.is_empty() && image.scale_set_name == expected.scale_set_name
        })
        && valid_allowed_group_workflows(expected)
        && !expected.policy_digest.is_empty()
}

fn valid_allowed_group_workflows(expected: &PoolBindingView<'_>) -> bool {
    let workflows = expected.allowed_group_workflows;
    if workflows.is_empty()
        || workflows.iter().any(|workflow| {
            !valid_group_workflow_identity(expected.target_repository_full_name, workflow)
        })
    {
        return false;
    }
    let mut ordered = workflows.iter().map(String::as_str).collect::<Vec<_>>();
    ordered.sort_unstable();
    !ordered.windows(2).any(|pair| pair[0] == pair[1])
}

pub(super) fn valid_group_workflow_identity(repository: &str, workflow: &str) -> bool {
    let Some(identity) = workflow.strip_prefix(&format!("{repository}/")) else {
        return false;
    };
    let Some((path, reference)) = identity.split_once('@') else {
        return false;
    };
    if reference.is_empty()
        || reference.contains('@')
        || reference.starts_with('/')
        || reference.ends_with('/')
        || reference.ends_with('.')
        || reference.contains("..")
        || reference.contains("//")
        || reference.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\' | b']')
        })
    {
        return false;
    }
    let Some(workflow_file) = path.strip_prefix(".github/workflows/") else {
        return false;
    };
    if workflow_file.is_empty()
        || workflow_file
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || workflow_file.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'?' | b'*' | b'[' | b'\\' | b']' | b'@')
        })
    {
        return false;
    }
    let Some(file_name) = workflow_file.rsplit('/').next() else {
        return false;
    };
    file_name
        .strip_suffix(".yaml")
        .or_else(|| file_name.strip_suffix(".yml"))
        .is_some_and(|stem| !stem.is_empty())
}

pub(super) fn validate_repository(
    expected: &PoolBindingView<'_>,
    repository: &RepositoryPoolTrustEvidence,
) -> Result<(), PoolAdmissionEvidence> {
    if repository.repository_id <= 0
        || expected
            .target_repository_id
            .is_some_and(|id| repository.repository_id != id)
        || !repository
            .repository_full_name
            .eq_ignore_ascii_case(expected.target_repository_full_name)
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    if !repository.private {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryNotPrivate,
        ));
    }
    if !repository.forks_disabled {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::ForkWorkflowsEnabled,
        ));
    }
    Ok(())
}

fn validate_route<'a>(
    expected: &PoolBindingView<'_>,
    organization: &str,
    route: &'a ActionsServiceScaleSetRoute,
) -> Result<&'a ScaleSetView, PoolAdmissionEvidence> {
    if !route.organization().eq_ignore_ascii_case(organization)
        || route.runner_group_id() != expected.actions_runner_group_id
        || route.runner_group_name() != expected.actions_runner_group_name
        || route.inventory_group_count() == 0
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    let scale_set = route.scale_set();
    if !valid_scale_set(scale_set, expected) {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    Ok(scale_set)
}

fn valid_scale_set(view: &ScaleSetView, expected: &PoolBindingView<'_>) -> bool {
    view.id > 0
        && expected.scale_set_id.is_none_or(|id| id == view.id)
        && view.name == expected.scale_set_name
        && view.runner_setting.disable_update
        && accept_scale_set_for(view, expected.scale_set_name).is_ok()
}

fn validate_group<'a>(
    expected: &PoolBindingView<'_>,
    organization: &str,
    repository_id: i64,
    evidence: &'a OrganizationRunnerGroupPolicyEvidence,
) -> Result<GroupPolicyFacts<'a>, PoolAdmissionEvidence> {
    let policy = evidence.policy();
    validate_group_scope(policy, organization)?;
    validate_group_identity(expected, evidence, policy)?;
    validate_group_restrictions(policy)?;
    let selected_repository_ids = selected_repository(policy, expected, repository_id)?;
    let selected_workflows = selected_workflows(policy)?;
    if !workflows_match(expected.allowed_group_workflows, selected_workflows) {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        ));
    }
    Ok(GroupPolicyFacts {
        policy,
        selected_repository_ids,
        selected_workflows,
    })
}

pub(super) fn validate_group_policy(
    expected: &PoolBindingView<'_>,
    organization: &str,
    repository_id: i64,
    evidence: &OrganizationRunnerGroupPolicyEvidence,
) -> Result<(), PoolAdmissionEvidence> {
    validate_group(expected, organization, repository_id, evidence).map(|_| ())
}

fn validate_group_scope(
    policy: &ActionsRunnerGroupPolicy,
    organization: &str,
) -> Result<(), PoolAdmissionEvidence> {
    match &policy.scope {
        RunnerGroupScope::Organization(actual) if actual.eq_ignore_ascii_case(organization) => {
            Ok(())
        }
        _ => Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        )),
    }
}

fn validate_group_identity(
    expected: &PoolBindingView<'_>,
    evidence: &OrganizationRunnerGroupPolicyEvidence,
    policy: &ActionsRunnerGroupPolicy,
) -> Result<(), PoolAdmissionEvidence> {
    if policy.id <= 0
        || policy.name != expected.actions_runner_group_name
        || evidence.inventory_group_count() == 0
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    if expected
        .rest_runner_group_id
        .is_some_and(|expected_id| expected_id != policy.id)
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    match policy.inherited {
        Some(false) => Ok(()),
        Some(true) => Err(PoolAdmissionEvidence::Unknown(
            PolicyGap::EffectiveRoutingApplicabilityUnproven,
        )),
        None => Err(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField)),
    }
}

fn validate_group_restrictions(
    policy: &ActionsRunnerGroupPolicy,
) -> Result<(), PoolAdmissionEvidence> {
    if policy.visibility != "selected" {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted,
        ));
    }
    match policy.allows_public_repositories {
        Some(false) => {}
        Some(true) => {
            return Err(PoolAdmissionEvidence::Rejected(
                PolicyMismatch::PublicRepositoriesAllowed,
            ));
        }
        None => return Err(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField)),
    }
    match policy.restricted_to_workflows {
        Some(true) => Ok(()),
        Some(false) => Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        )),
        None => Err(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField)),
    }
}

fn selected_repository(
    policy: &ActionsRunnerGroupPolicy,
    expected: &PoolBindingView<'_>,
    repository_id: i64,
) -> Result<Vec<i64>, PoolAdmissionEvidence> {
    let RunnerGroupAccess::SelectedRepositories(repositories) = &policy.access else {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted,
        ));
    };
    if repositories.len() != 1
        || repositories[0].id != repository_id
        || expected
            .target_repository_id
            .is_some_and(|id| repositories[0].id != id)
        || !repositories[0]
            .full_name
            .eq_ignore_ascii_case(expected.target_repository_full_name)
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted,
        ));
    }
    match repositories[0].private {
        Some(true) => Ok(vec![repositories[0].id]),
        Some(false) => Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryNotPrivate,
        )),
        None => Err(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField)),
    }
}

fn selected_workflows(
    policy: &ActionsRunnerGroupPolicy,
) -> Result<&[String], PoolAdmissionEvidence> {
    let Some(workflows) = policy.selected_workflows.as_deref() else {
        return Err(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField));
    };
    if workflows.is_empty() {
        return Err(PoolAdmissionEvidence::Unknown(
            PolicyGap::IncompletePolicyRead,
        ));
    }
    Ok(workflows)
}

fn workflows_match(configured: &[String], selected: &[String]) -> bool {
    if configured.is_empty() || selected.is_empty() {
        return false;
    }
    let mut configured = configured.iter().map(String::as_str).collect::<Vec<_>>();
    let mut selected = selected.iter().map(String::as_str).collect::<Vec<_>>();
    configured.sort_unstable();
    selected.sort_unstable();
    !configured.windows(2).any(|pair| pair[0] == pair[1])
        && !selected.windows(2).any(|pair| pair[0] == pair[1])
        && configured == selected
}

#[cfg(test)]
mod tests;
