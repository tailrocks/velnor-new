use std::time::SystemTime;

use crate::{ActionsServiceScaleSetRoute, OrganizationRunnerGroupPolicyEvidence, ScaleSetView};

use super::super::types::{
    PoolBinding, PoolBindingView, PoolEvidenceSource, PoolPolicySnapshot, PoolRegistrationScope,
    RunnerImageIdentity, WorkflowTrustField,
};
use super::organization::GroupPolicyFacts;
use super::repository::RepositoryPoolTrustEvidence;
use super::{EffectiveRoutingProof, source_stamp};

const ORG_GROUP_ROUTE_CONTRACT: &str =
    "github-org-runner-group-name-route-arc-5aed393-scaleset-e6daac7";

pub(super) fn build_snapshot(
    expected: &PoolBindingView<'_>,
    organization: &str,
    repository: &RepositoryPoolTrustEvidence,
    runner_group: &OrganizationRunnerGroupPolicyEvidence,
    route: &ActionsServiceScaleSetRoute,
    scale_set: &ScaleSetView,
    facts: &GroupPolicyFacts<'_>,
) -> PoolPolicySnapshot {
    let binding = PoolBinding {
        registration_scope: PoolRegistrationScope::Organization {
            organization: organization.to_owned(),
        },
        repository_id: repository.repository_id,
        repository_full_name: repository.repository_full_name.clone(),
        scale_set_id: scale_set.id,
        scale_set_name: scale_set.name.clone(),
        actions_runner_group_id: route.runner_group_id(),
        actions_runner_group_name: route.runner_group_name().to_owned(),
        rest_runner_group_id: Some(facts.policy.id),
        runner_image_profile: expected.runner_image.map(|image| image.profile.to_owned()),
        runner_image: expected.runner_image.map(RunnerImageIdentity::from),
        policy_digest: expected.policy_digest.to_owned(),
    };
    PoolPolicySnapshot {
        binding: WorkflowTrustField::Present(binding),
        repository_private: WorkflowTrustField::Present(repository.private),
        forks_disabled: WorkflowTrustField::Present(repository.forks_disabled),
        group_visibility: WorkflowTrustField::Present(facts.policy.visibility.clone()),
        allows_public_repositories: WorkflowTrustField::Present(
            facts.policy.allows_public_repositories.unwrap_or(true),
        ),
        workflow_restrictions_enabled: WorkflowTrustField::Present(
            facts.policy.restricted_to_workflows.unwrap_or(false),
        ),
        selected_repository_ids: WorkflowTrustField::Present(facts.selected_repository_ids.clone()),
        selected_workflows: WorkflowTrustField::Present(facts.selected_workflows.to_vec()),
        pages_complete: true,
        sources: vec![
            source_stamp(
                PoolEvidenceSource::RepositoryMetadataRest,
                repository.repository_observed_at,
                crate::actions::API_VERSION,
            ),
            source_stamp(
                PoolEvidenceSource::RepositoryForkPolicyRest,
                repository.fork_policy_observed_at,
                crate::actions::API_VERSION,
            ),
            source_stamp(
                PoolEvidenceSource::OrganizationRunnerGroupRest,
                runner_group.observed_at(),
                crate::actions::API_VERSION,
            ),
            source_stamp(
                PoolEvidenceSource::ScaleSetServiceRest,
                route.observed_at(),
                crate::session::API_QUERY,
            ),
        ],
    }
}

pub(super) fn effective_route_proof(
    expected: &PoolBindingView<'_>,
    organization: &str,
    repository: &RepositoryPoolTrustEvidence,
    runner_group: &OrganizationRunnerGroupPolicyEvidence,
    route: &ActionsServiceScaleSetRoute,
    scale_set: &ScaleSetView,
    now: SystemTime,
) -> EffectiveRoutingProof {
    let observed_at = [
        repository.repository_observed_at,
        repository.fork_policy_observed_at,
        runner_group.observed_at(),
        route.observed_at(),
    ]
    .into_iter()
    .min()
    .unwrap_or(now);
    EffectiveRoutingProof {
        contract_id: ORG_GROUP_ROUTE_CONTRACT.to_owned(),
        registration_scope: PoolRegistrationScope::Organization {
            organization: organization.to_owned(),
        },
        rest_runner_group_id: runner_group.policy().id,
        actions_runner_group_id: route.runner_group_id(),
        scale_set_id: scale_set.id,
        observed_at,
        policy_digest: expected.policy_digest.to_owned(),
    }
}
