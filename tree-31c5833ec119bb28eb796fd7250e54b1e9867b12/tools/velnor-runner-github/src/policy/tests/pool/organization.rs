use std::sync::OnceLock;
use std::time::SystemTime;

use super::super::common::POLICY_DIGEST;
use crate::policy::{
    PolicyGap, PolicyMismatch, PoolAdmissionEvidence, PoolBindingView, PoolRegistrationScopeView,
    RepositoryPoolTrustEvidence, RunnerImageIdentityView, verify_organization_pool_policy,
};
use crate::{
    ActionsRunnerGroupPolicy, ActionsServiceScaleSetRoute, Label,
    OrganizationRunnerGroupPolicyEvidence, RunnerGroup, RunnerGroupAccess,
    RunnerGroupPolicySnapshot, RunnerGroupScope, ScaleSetView, SelectedRepository,
};

fn organization_binding_view() -> PoolBindingView<'static> {
    static WORKFLOWS: OnceLock<Vec<String>> = OnceLock::new();
    let allowed_group_workflows = WORKFLOWS.get_or_init(|| {
        vec!["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main".to_owned()]
    });
    PoolBindingView {
        registration_scope: PoolRegistrationScopeView::Organization {
            organization: "ChainArgos",
        },
        target_repository_id: Some(829_618_808),
        target_repository_full_name: "ChainArgos/java-monorepo",
        scale_set_id: Some(35),
        scale_set_name: "ubuntu-24.04-scale-set",
        actions_runner_group_id: 13,
        actions_runner_group_name: "velnor-trusted",
        rest_runner_group_id: Some(8),
        runner_image: Some(test_image()),
        allowed_group_workflows,
        policy_digest: POLICY_DIGEST,
    }
}

fn test_image() -> RunnerImageIdentityView<'static> {
    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    RunnerImageIdentityView {
        profile: "ubuntu-24.04-amd64",
        scale_set_name: "ubuntu-24.04-scale-set",
        platform: "linux/amd64",
        runner_image: "ghcr.io/actions/actions-runner@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        runner_manifest_digest: DIGEST,
        runner_index_digest: DIGEST,
        runner_config_digest: DIGEST,
        runner_os: "ubuntu24",
        runner_release_version: "2.338.0",
        runner_release_published_at: "2026-10-06T13:55:11Z",
        runner_requalify_by: "2099-11-05T13:55:11Z",
        dind_image: "docker.io/library/docker@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        dind_manifest_digest: DIGEST,
        dind_index_digest: DIGEST,
        dind_config_digest: DIGEST,
        dind_version: "29.8.2",
        dind_source: "docker-library/docker@0123456789abcdef0123456789abcdef01234567",
        dind_entrypoint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    }
}

fn organization_evidence(
    expected: &PoolBindingView<'_>,
    now: SystemTime,
) -> (
    RepositoryPoolTrustEvidence,
    OrganizationRunnerGroupPolicyEvidence,
    ActionsServiceScaleSetRoute,
) {
    let repository = RepositoryPoolTrustEvidence::from_test_parts(
        expected.target_repository_id.unwrap_or(829_618_808),
        expected.target_repository_full_name.to_owned(),
        true,
        true,
        now,
        now,
    );
    let policy = ActionsRunnerGroupPolicy {
        scope: RunnerGroupScope::Organization("ChainArgos".to_owned()),
        id: 8,
        name: "velnor-trusted".to_owned(),
        visibility: "selected".to_owned(),
        is_default: Some(false),
        inherited: Some(false),
        allows_public_repositories: Some(false),
        restricted_to_workflows: Some(true),
        selected_workflows: Some(expected.allowed_group_workflows.to_vec()),
        workflow_restrictions_read_only: Some(false),
        access: RunnerGroupAccess::SelectedRepositories(vec![SelectedRepository {
            id: expected.target_repository_id.unwrap_or(829_618_808),
            name: "java-monorepo".to_owned(),
            full_name: expected.target_repository_full_name.to_owned(),
            private: Some(true),
        }]),
    };
    let group_evidence = OrganizationRunnerGroupPolicyEvidence::from_test_snapshot(
        RunnerGroupPolicySnapshot {
            policy,
            inventory_group_count: 2,
        },
        now,
    );
    let route = ActionsServiceScaleSetRoute::from_test_parts(
        "ChainArgos".to_owned(),
        2,
        RunnerGroup {
            id: expected.actions_runner_group_id,
            name: expected.actions_runner_group_name.to_owned(),
            is_default: false,
        },
        ScaleSetView {
            id: 35,
            name: expected.scale_set_name.to_owned(),
            labels: vec![
                Label {
                    name: "velnor".to_owned(),
                    label_type: "System".to_owned(),
                },
                Label {
                    name: expected.scale_set_name.to_owned(),
                    label_type: "System".to_owned(),
                },
            ],
            runner_setting: crate::registration::RunnerSetting {
                disable_update: true,
            },
        },
        now,
    );
    (repository, group_evidence, route)
}

#[test]
fn ubuntu24_profile_never_issues_ubuntu26_admission() {
    let now = SystemTime::now();
    let expected = organization_binding_view();
    let (repository, group, route) = organization_evidence(&expected, now);
    let result = verify_organization_pool_policy(&expected, &repository, &group, &route, now);
    assert_eq!(
        result,
        PoolAdmissionEvidence::Unknown(PolicyGap::RequiredRunnerProfileUnavailable)
    );
}

#[test]
fn organization_issuer_rejects_group_repository_id_mismatch_without_expected_id() {
    let now = SystemTime::now();
    let base = organization_binding_view();
    let expected = PoolBindingView {
        target_repository_id: None,
        ..base
    };
    let (repository, group, route) = organization_evidence(&expected, now);
    let mut policy = group.policy().clone();
    let RunnerGroupAccess::SelectedRepositories(repositories) = &mut policy.access else {
        panic!("test policy must have selected repositories");
    };
    repositories[0].id += 1;
    let mismatched_group = OrganizationRunnerGroupPolicyEvidence::from_test_snapshot(
        RunnerGroupPolicySnapshot {
            policy,
            inventory_group_count: group.inventory_group_count(),
        },
        now,
    );

    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &mismatched_group, &route, now,),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::RepositoryRoutingUnrestricted)
    );
}

#[test]
fn organization_issuer_requires_the_exact_configured_workflow_set() {
    let now = SystemTime::now();
    let expected = organization_binding_view();
    let (repository, group, route) = organization_evidence(&expected, now);
    let mut policy = group.policy().clone();
    policy
        .selected_workflows
        .as_mut()
        .expect("selected workflow policy")
        .push("ChainArgos/java-monorepo/.github/workflows/release.yml@main".to_owned());
    let expanded_group = OrganizationRunnerGroupPolicyEvidence::from_test_snapshot(
        RunnerGroupPolicySnapshot {
            policy,
            inventory_group_count: group.inventory_group_count(),
        },
        now,
    );
    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &expanded_group, &route, now),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::WorkflowRoutingMismatch)
    );

    let mut policy = group.policy().clone();
    policy
        .selected_workflows
        .as_mut()
        .expect("selected workflow policy")
        .push(expected.allowed_group_workflows[0].clone());
    let duplicate_group = OrganizationRunnerGroupPolicyEvidence::from_test_snapshot(
        RunnerGroupPolicySnapshot {
            policy,
            inventory_group_count: group.inventory_group_count(),
        },
        now,
    );
    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &duplicate_group, &route, now),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::WorkflowRoutingMismatch)
    );
}

#[test]
fn organization_issuer_rejects_non_exact_workflow_identities() {
    for workflow in [
        "ChainArgos/java-monorepo/.github/workflows/ci.yml",
        "ChainArgos/java-monorepo/.github/workflows/*@refs/heads/main",
        "ChainArgos/java-monorepo/@refs/heads/main",
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@",
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/*",
        "ChainArgos/java-monorepo/.github/workflows/../release.yml@refs/heads/main",
    ] {
        let now = SystemTime::now();
        let base = organization_binding_view();
        let allowed_group_workflows = [workflow.to_owned()];
        let expected = PoolBindingView {
            allowed_group_workflows: &allowed_group_workflows,
            ..base
        };
        let (repository, group, route) = organization_evidence(&expected, now);
        assert_eq!(
            verify_organization_pool_policy(&expected, &repository, &group, &route, now),
            PoolAdmissionEvidence::Rejected(PolicyMismatch::InvalidPolicy),
            "workflow identity must be exact: {workflow}"
        );
    }
}

#[test]
fn organization_issuer_keeps_missing_exact_workflow_policy_unknown() {
    let now = SystemTime::now();
    let base = organization_binding_view();
    let expected = PoolBindingView {
        allowed_group_workflows: &[],
        ..base
    };
    let (repository, group, route) = organization_evidence(&expected, now);
    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &group, &route, now),
        PoolAdmissionEvidence::Unknown(PolicyGap::WorkflowRuleSetEmpty)
    );
}

#[test]
fn current_runner_profile_does_not_qualify_ubuntu26_admission() {
    let now = SystemTime::now();
    let base = organization_binding_view();
    let stale_image = RunnerImageIdentityView {
        runner_requalify_by: "2026-10-07T13:55:11Z",
        ..base.runner_image.expect("organization profile is required")
    };
    let expected = PoolBindingView {
        runner_image: Some(stale_image),
        ..base
    };
    let (repository, group, route) = organization_evidence(&expected, now);
    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &group, &route, now),
        PoolAdmissionEvidence::Unknown(PolicyGap::RequiredRunnerProfileUnavailable)
    );
}

#[test]
fn organization_issuer_rejects_unrestricted_policy_and_keeps_repo_scope_unknown() {
    let now = SystemTime::now();
    let expected = organization_binding_view();
    let (repository, group, route) = organization_evidence(&expected, now);

    let mut policy = group.policy().clone();
    policy.restricted_to_workflows = Some(false);
    let unrestricted = OrganizationRunnerGroupPolicyEvidence::from_test_snapshot(
        RunnerGroupPolicySnapshot {
            policy,
            inventory_group_count: group.inventory_group_count(),
        },
        now,
    );
    assert_eq!(
        verify_organization_pool_policy(&expected, &repository, &unrestricted, &route, now),
        PoolAdmissionEvidence::Rejected(PolicyMismatch::WorkflowRoutingMismatch)
    );

    let repository_scope = PoolBindingView {
        registration_scope: PoolRegistrationScopeView::Repository {
            owner: "ChainArgos",
            repository: "java-monorepo",
        },
        ..expected
    };
    assert_eq!(
        verify_organization_pool_policy(&repository_scope, &repository, &group, &route, now),
        PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven)
    );
}
