use std::time::{Duration, SystemTime};

use super::common::POLICY_DIGEST;
use crate::policy::{
    PolicyGap, PolicyMismatch, PoolAdmissionEvidence, PoolBinding, PoolBindingView,
    PoolEvidenceSource, PoolEvidenceSourceStamp, PoolPolicySnapshot, WorkflowTrustField,
    verify_pool_policy,
};

fn binding_view() -> PoolBindingView<'static> {
    PoolBindingView {
        repository_id: 829_618_808,
        repository_full_name: "ChainArgos/java-monorepo",
        scale_set_id: 3,
        scale_set_name: "ubuntu-26.04-scale-set",
        runner_group_id: 1,
        runner_group_name: "Default",
        policy_digest: POLICY_DIGEST,
    }
}

fn snapshot(expected: PoolBindingView<'_>, now: SystemTime) -> PoolPolicySnapshot {
    let sources = [
        PoolEvidenceSource::RepositoryMetadataRest,
        PoolEvidenceSource::RepositoryForkPolicyRest,
        PoolEvidenceSource::ScaleSetServiceRest,
        PoolEvidenceSource::OrganizationRunnerGroupRest,
    ]
    .map(|source| PoolEvidenceSourceStamp {
        source,
        observed_at: now,
        source_version: "2026-03-10".to_owned(),
    })
    .to_vec();
    PoolPolicySnapshot {
        binding: WorkflowTrustField::Present(PoolBinding {
            repository_id: expected.repository_id,
            repository_full_name: expected.repository_full_name.to_owned(),
            scale_set_id: expected.scale_set_id,
            scale_set_name: expected.scale_set_name.to_owned(),
            runner_group_id: expected.runner_group_id,
            runner_group_name: expected.runner_group_name.to_owned(),
            policy_digest: expected.policy_digest.to_owned(),
        }),
        repository_private: WorkflowTrustField::Present(true),
        forks_disabled: WorkflowTrustField::Present(true),
        group_visibility: WorkflowTrustField::Present("selected".to_owned()),
        allows_public_repositories: WorkflowTrustField::Present(false),
        workflow_restrictions_enabled: WorkflowTrustField::Present(true),
        selected_repository_ids: WorkflowTrustField::Present(vec![expected.repository_id]),
        selected_workflows: WorkflowTrustField::Present(vec![
            ".github/workflows/ci.yml@main".to_owned(),
        ]),
        pages_complete: true,
        sources,
    }
}

#[test]
fn consistent_metadata_alone_never_proves_effective_routing() {
    let now = SystemTime::now();
    assert_eq!(
        verify_pool_policy(&binding_view(), &snapshot(binding_view(), now), now),
        PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven)
    );
}

#[test]
fn public_or_unrestricted_settings_are_rejected_and_stale_reads_unknown() {
    let now = SystemTime::now();
    let expected = binding_view();
    let base = snapshot(expected, now);

    let mut public = base.clone();
    public.repository_private = WorkflowTrustField::Present(false);
    assert_eq!(
        verify_pool_policy(&expected, &public, now),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::RepositoryNotPrivate)
    );

    let mut unrestricted = base.clone();
    unrestricted.group_visibility = WorkflowTrustField::Present("all".to_owned());
    assert_eq!(
        verify_pool_policy(&expected, &unrestricted, now),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::RepositoryRoutingUnrestricted)
    );

    let mut stale = base;
    for source in &mut stale.sources {
        source.observed_at = now - Duration::from_secs(31);
    }
    assert_eq!(
        verify_pool_policy(&expected, &stale, now),
        PoolAdmissionEvidence::Unknown(PolicyGap::StaleEvidence)
    );
}
