use std::time::{Duration, SystemTime};

mod evidence;
mod image;
mod organization;
mod preflight;
mod repository;
mod snapshot;

use super::types::{
    PolicyGap, PolicyMismatch, PoolBinding, PoolBindingView, PoolEvidenceSource,
    PoolEvidenceSourceStamp, PoolPolicySnapshot, PoolRegistrationScope, PoolRegistrationScopeView,
    RunnerImageIdentity, WorkflowTrustField,
};
pub use evidence::{PoolAdmissionEvidence, VerifiedPoolPolicy};
pub use organization::verify_organization_pool_policy;
pub use preflight::preflight_organization_pool_admission_async;
pub use repository::{
    RepositoryPoolTrustEvidence, read_repository_pool_trust, read_repository_pool_trust_async,
};

const MAX_POLICY_AGE: Duration = Duration::from_secs(30);

/// Private contract marker required before pool metadata can authorize
/// admission. No current public reader can issue this evidence.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct EffectiveRoutingProof {
    contract_id: String,
    registration_scope: PoolRegistrationScope,
    rest_runner_group_id: i64,
    actions_runner_group_id: i64,
    scale_set_id: i64,
    observed_at: SystemTime,
    policy_digest: String,
}

/// Metadata consistency plus freshness is not enough to establish an
/// effective route. This function intentionally returns `Unknown` for every
/// otherwise-consistent snapshot until a source-specific verifier produces a
/// private [`EffectiveRoutingProof`].
#[must_use]
pub fn verify_pool_policy(
    expected: &PoolBindingView<'_>,
    observed: &PoolPolicySnapshot,
    now: SystemTime,
) -> PoolAdmissionEvidence {
    verify_pool_policy_with_proof(expected, observed, None, now)
}

fn verify_pool_policy_with_proof(
    expected: &PoolBindingView<'_>,
    observed: &PoolPolicySnapshot,
    routing_proof: Option<&EffectiveRoutingProof>,
    now: SystemTime,
) -> PoolAdmissionEvidence {
    let binding = match validate_pool_identity(expected, observed) {
        Ok(binding) => binding,
        Err(evidence) => return evidence,
    };
    if let Err(evidence) = validate_pool_routing(expected, observed, now) {
        return evidence;
    }

    // Group metadata does not prove that the repository-scoped Scale Set uses
    // the same routing policy. Keep admission fenced without source proof.
    let Some(routing_proof) = routing_proof else {
        return PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven);
    };
    if routing_proof.contract_id.is_empty()
        || routing_proof.registration_scope != binding.registration_scope
        || Some(routing_proof.rest_runner_group_id) != binding.rest_runner_group_id
        || routing_proof.actions_runner_group_id != binding.actions_runner_group_id
        || routing_proof.scale_set_id != binding.scale_set_id
        || routing_proof.policy_digest != expected.policy_digest
        || !fresh(routing_proof.observed_at, now)
    {
        return PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven);
    }
    let Some(image) = expected.runner_image else {
        return PoolAdmissionEvidence::Unknown(PolicyGap::MissingField);
    };
    if !image::has_required_admission_profile(&image, expected.scale_set_name) {
        return PoolAdmissionEvidence::Unknown(PolicyGap::RequiredRunnerProfileUnavailable);
    }
    let Some(image_deadline) = image::validate_image_profile(&image, expected.scale_set_name)
    else {
        return PoolAdmissionEvidence::Unknown(PolicyGap::RequiredRunnerProfileUnavailable);
    };
    if image_deadline <= now {
        return PoolAdmissionEvidence::Unknown(PolicyGap::StaleImageProfile);
    }
    let expires_at = match evidence_expiry(
        &observed.sources,
        routing_proof.observed_at,
        image_deadline,
        now,
    ) {
        Ok(expires_at) => expires_at,
        Err(evidence) => return evidence,
    };
    PoolAdmissionEvidence::Verified(Box::new(VerifiedPoolPolicy {
        binding: binding.clone(),
        policy_digest: expected.policy_digest.to_owned(),
        verified_at: now,
        expires_at,
        _routing_proof: EffectiveRoutingProof {
            contract_id: routing_proof.contract_id.clone(),
            registration_scope: routing_proof.registration_scope.clone(),
            rest_runner_group_id: routing_proof.rest_runner_group_id,
            actions_runner_group_id: routing_proof.actions_runner_group_id,
            scale_set_id: routing_proof.scale_set_id,
            observed_at: routing_proof.observed_at,
            policy_digest: routing_proof.policy_digest.clone(),
        },
    }))
}

fn evidence_expiry(
    sources: &[PoolEvidenceSourceStamp],
    routing_observed_at: SystemTime,
    image_deadline: SystemTime,
    now: SystemTime,
) -> Result<SystemTime, PoolAdmissionEvidence> {
    let Some(oldest_source_observation) = sources.iter().map(|stamp| stamp.observed_at).min()
    else {
        return Err(PoolAdmissionEvidence::Unknown(
            PolicyGap::IncompletePolicyRead,
        ));
    };
    let oldest_observation = oldest_source_observation.min(routing_observed_at);
    let Some(policy_expiry) = oldest_observation.checked_add(MAX_POLICY_AGE) else {
        return Err(PoolAdmissionEvidence::Unknown(PolicyGap::InvalidField));
    };
    let expires_at = policy_expiry.min(image_deadline);
    if expires_at <= now {
        return Err(PoolAdmissionEvidence::Unknown(PolicyGap::StaleEvidence));
    }
    Ok(expires_at)
}

fn validate_pool_identity<'a>(
    expected: &PoolBindingView<'_>,
    observed: &'a PoolPolicySnapshot,
) -> Result<&'a PoolBinding, PoolAdmissionEvidence> {
    if expected.target_repository_id.is_some_and(|id| id <= 0)
        || expected.scale_set_id.is_some_and(|id| id <= 0)
        || expected.actions_runner_group_id <= 0
        || expected.target_repository_full_name.is_empty()
        || expected.scale_set_name.is_empty()
        || expected.actions_runner_group_name.is_empty()
        || expected.policy_digest.is_empty()
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::InvalidPolicy,
        ));
    }
    let Some(binding) = present(&observed.binding) else {
        return Err(gap_for(&observed.binding, PolicyGap::MissingField));
    };
    if !scope_matches(&binding.registration_scope, expected.registration_scope)
        || expected
            .target_repository_id
            .is_some_and(|id| binding.repository_id != id)
        || !binding
            .repository_full_name
            .eq_ignore_ascii_case(expected.target_repository_full_name)
        || expected
            .scale_set_id
            .is_some_and(|scale_set_id| binding.scale_set_id != scale_set_id)
        || binding.scale_set_name != expected.scale_set_name
        || binding.actions_runner_group_id != expected.actions_runner_group_id
        || binding.actions_runner_group_name != expected.actions_runner_group_name
        || expected
            .rest_runner_group_id
            .is_some_and(|id| binding.rest_runner_group_id != Some(id))
        || binding.runner_image_profile.as_deref()
            != expected.runner_image.map(|image| image.profile)
        || binding.runner_image != expected.runner_image.map(RunnerImageIdentity::from)
        || binding.policy_digest != expected.policy_digest
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    let Some(is_private) = present(&observed.repository_private) else {
        return Err(gap_for(
            &observed.repository_private,
            PolicyGap::MissingField,
        ));
    };
    if !is_private {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryNotPrivate,
        ));
    }
    let Some(forks_disabled) = present(&observed.forks_disabled) else {
        return Err(gap_for(&observed.forks_disabled, PolicyGap::MissingField));
    };
    if !forks_disabled {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::ForkWorkflowsEnabled,
        ));
    }
    Ok(binding)
}

fn validate_pool_routing(
    expected: &PoolBindingView<'_>,
    observed: &PoolPolicySnapshot,
    now: SystemTime,
) -> Result<(), PoolAdmissionEvidence> {
    let Some(visibility) = present(&observed.group_visibility) else {
        return Err(gap_for(&observed.group_visibility, PolicyGap::MissingField));
    };
    if visibility != "selected" {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted,
        ));
    }
    let Some(allows_public) = present(&observed.allows_public_repositories) else {
        return Err(gap_for(
            &observed.allows_public_repositories,
            PolicyGap::MissingField,
        ));
    };
    if *allows_public {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PublicRepositoriesAllowed,
        ));
    }
    let Some(workflow_restrictions) = present(&observed.workflow_restrictions_enabled) else {
        return Err(gap_for(
            &observed.workflow_restrictions_enabled,
            PolicyGap::MissingField,
        ));
    };
    if !workflow_restrictions {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        ));
    }
    let Some(selected_repositories) = present(&observed.selected_repository_ids) else {
        return Err(gap_for(
            &observed.selected_repository_ids,
            PolicyGap::MissingField,
        ));
    };
    if selected_repositories.len() != 1
        || expected
            .target_repository_id
            .is_some_and(|id| selected_repositories[0] != id)
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted,
        ));
    }
    let Some(selected_workflows) = present(&observed.selected_workflows) else {
        return Err(gap_for(
            &observed.selected_workflows,
            PolicyGap::MissingField,
        ));
    };
    if selected_workflows.is_empty() || expected.allowed_group_workflows.is_empty() {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        ));
    }
    let mut actual_workflows = selected_workflows.clone();
    let mut expected_workflows = expected.allowed_group_workflows.to_vec();
    actual_workflows.sort();
    expected_workflows.sort();
    if actual_workflows != expected_workflows
        || expected_workflows.windows(2).any(|pair| pair[0] == pair[1])
        || expected_workflows.iter().any(|workflow| {
            !organization::valid_group_workflow_identity(
                expected.target_repository_full_name,
                workflow,
            )
        })
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        ));
    }
    validate_source_completeness_and_freshness(observed, now)
}

fn validate_source_completeness_and_freshness(
    observed: &PoolPolicySnapshot,
    now: SystemTime,
) -> Result<(), PoolAdmissionEvidence> {
    if !observed.pages_complete {
        return Err(PoolAdmissionEvidence::Unknown(
            PolicyGap::IncompletePolicyRead,
        ));
    }
    if !sources_fresh(&observed.sources, now) {
        return Err(PoolAdmissionEvidence::Unknown(PolicyGap::StaleEvidence));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn scope_matches(
    expected: &PoolRegistrationScope,
    observed: PoolRegistrationScopeView<'_>,
) -> bool {
    match (expected, observed) {
        (
            PoolRegistrationScope::Repository {
                owner: expected_owner,
                repository: expected_repository,
            },
            PoolRegistrationScopeView::Repository { owner, repository },
        ) => {
            expected_owner.eq_ignore_ascii_case(owner)
                && expected_repository.eq_ignore_ascii_case(repository)
        }
        (
            PoolRegistrationScope::Organization {
                organization: expected_organization,
            },
            PoolRegistrationScopeView::Organization { organization },
        ) => expected_organization.eq_ignore_ascii_case(organization),
        _ => false,
    }
}

fn sources_fresh(sources: &[PoolEvidenceSourceStamp], now: SystemTime) -> bool {
    use PoolEvidenceSource as Source;

    let required = [
        Source::RepositoryMetadataRest,
        Source::RepositoryForkPolicyRest,
        Source::ScaleSetServiceRest,
        Source::OrganizationRunnerGroupRest,
    ];
    required.iter().all(|required_source| {
        let matching: Vec<_> = sources
            .iter()
            .filter(|stamp| stamp.source == *required_source)
            .collect();
        if matching.len() != 1 || matching[0].source_version.is_empty() {
            return false;
        }
        fresh(matching[0].observed_at, now)
    })
}

fn source_stamp(
    source: PoolEvidenceSource,
    observed_at: SystemTime,
    source_version: &str,
) -> PoolEvidenceSourceStamp {
    PoolEvidenceSourceStamp {
        source,
        observed_at,
        source_version: source_version.to_owned(),
    }
}

fn fresh(observed_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(observed_at)
        .is_ok_and(|age| age <= MAX_POLICY_AGE)
}

fn present<T>(field: &WorkflowTrustField<T>) -> Option<&T> {
    match field {
        WorkflowTrustField::Present(value) => Some(value),
        WorkflowTrustField::Missing | WorkflowTrustField::Invalid => None,
    }
}

fn gap_for<T>(field: &WorkflowTrustField<T>, missing: PolicyGap) -> PoolAdmissionEvidence {
    PoolAdmissionEvidence::Unknown(match field {
        WorkflowTrustField::Missing => missing,
        WorkflowTrustField::Invalid => PolicyGap::InvalidField,
        WorkflowTrustField::Present(_) => unreachable!("present field has a value"),
    })
}
