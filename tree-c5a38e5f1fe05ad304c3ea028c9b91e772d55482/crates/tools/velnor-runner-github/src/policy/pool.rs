use std::time::{Duration, SystemTime};

mod evidence;

use super::types::{
    PolicyGap, PolicyMismatch, PoolBinding, PoolBindingView, PoolEvidenceSource,
    PoolEvidenceSourceStamp, PoolPolicySnapshot, WorkflowTrustField,
};
pub use evidence::{PoolAdmissionEvidence, VerifiedPoolPolicy};

const MAX_POLICY_AGE: Duration = Duration::from_secs(30);

/// Private contract marker required before pool metadata can authorize
/// admission. No current public reader can issue this evidence.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct EffectiveRoutingProof {
    contract_id: String,
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
        || routing_proof.policy_digest != expected.policy_digest
        || !fresh(routing_proof.observed_at, now)
    {
        return PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven);
    }
    let Some(expires_at) = now.checked_add(MAX_POLICY_AGE) else {
        return PoolAdmissionEvidence::Unknown(PolicyGap::InvalidField);
    };
    PoolAdmissionEvidence::Verified(Box::new(VerifiedPoolPolicy {
        binding: binding.clone(),
        policy_digest: expected.policy_digest.to_owned(),
        verified_at: now,
        expires_at,
        _routing_proof: EffectiveRoutingProof {
            contract_id: routing_proof.contract_id.clone(),
            observed_at: routing_proof.observed_at,
            policy_digest: routing_proof.policy_digest.clone(),
        },
    }))
}

fn validate_pool_identity<'a>(
    expected: &PoolBindingView<'_>,
    observed: &'a PoolPolicySnapshot,
) -> Result<&'a PoolBinding, PoolAdmissionEvidence> {
    if expected.repository_id <= 0
        || expected.scale_set_id <= 0
        || expected.runner_group_id <= 0
        || expected.repository_full_name.is_empty()
        || expected.scale_set_name.is_empty()
        || expected.runner_group_name.is_empty()
        || expected.policy_digest.is_empty()
    {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::InvalidPolicy,
        ));
    }
    let Some(binding) = present(&observed.binding) else {
        return Err(gap_for(&observed.binding, PolicyGap::MissingField));
    };
    if binding.repository_id != expected.repository_id
        || !binding
            .repository_full_name
            .eq_ignore_ascii_case(expected.repository_full_name)
        || binding.scale_set_id != expected.scale_set_id
        || binding.scale_set_name != expected.scale_set_name
        || binding.runner_group_id != expected.runner_group_id
        || binding.runner_group_name != expected.runner_group_name
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
    if selected_repositories.as_slice() != [expected.repository_id] {
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
    if selected_workflows.is_empty() {
        return Err(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::WorkflowRoutingMismatch,
        ));
    }
    if !observed.pages_complete || !sources_fresh(&observed.sources, now) {
        return Err(PoolAdmissionEvidence::Unknown(if observed.pages_complete {
            PolicyGap::StaleEvidence
        } else {
            PolicyGap::IncompletePolicyRead
        }));
    }
    Ok(())
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
