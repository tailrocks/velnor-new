use crate::policy::{PolicyMismatch, PoolBindingView, PoolRegistrationScopeView};
use crate::{ActionsRunnerGroupPolicy, RunnerGroupAccess, RunnerGroupScope, SelectedRepository};

use super::{selected_repository, valid_group_workflow_identity};

fn binding_without_repository_id(workflows: &[String]) -> PoolBindingView<'_> {
    PoolBindingView {
        registration_scope: PoolRegistrationScopeView::Organization {
            organization: "ChainArgos",
        },
        target_repository_id: None,
        target_repository_full_name: "ChainArgos/java-monorepo",
        scale_set_id: None,
        scale_set_name: "ubuntu-24.04-scale-set",
        actions_runner_group_id: 13,
        actions_runner_group_name: "velnor-trusted",
        rest_runner_group_id: None,
        runner_image: None,
        allowed_group_workflows: workflows,
        policy_digest: "test-digest",
    }
}

fn selected_repository_policy(id: i64) -> ActionsRunnerGroupPolicy {
    ActionsRunnerGroupPolicy {
        scope: RunnerGroupScope::Organization("ChainArgos".to_owned()),
        id: 8,
        name: "velnor-trusted".to_owned(),
        visibility: "selected".to_owned(),
        is_default: Some(false),
        inherited: Some(false),
        allows_public_repositories: Some(false),
        restricted_to_workflows: Some(true),
        selected_workflows: Some(vec![
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        ]),
        workflow_restrictions_read_only: Some(false),
        access: RunnerGroupAccess::SelectedRepositories(vec![SelectedRepository {
            id,
            name: "java-monorepo".to_owned(),
            full_name: "ChainArgos/java-monorepo".to_owned(),
            private: Some(true),
        }]),
    }
}

#[test]
fn selected_repository_id_is_always_bound_to_repository_evidence() {
    let expected = binding_without_repository_id(&[]);
    assert_eq!(
        selected_repository(
            &selected_repository_policy(829_618_809),
            &expected,
            829_618_808
        ),
        Err(crate::policy::PoolAdmissionEvidence::Rejected(
            PolicyMismatch::RepositoryRoutingUnrestricted
        ))
    );
    assert_eq!(
        selected_repository(
            &selected_repository_policy(829_618_808),
            &expected,
            829_618_808
        ),
        Ok(vec![829_618_808])
    );
}

#[test]
fn group_workflow_identity_requires_exact_workflow_path_and_reference() {
    let repository = "ChainArgos/java-monorepo";
    assert!(valid_group_workflow_identity(
        repository,
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"
    ));
    for workflow in [
        "ChainArgos/java-monorepo/.github/workflows/ci.yml",
        "ChainArgos/java-monorepo/.github/workflows/*@refs/heads/main",
        "ChainArgos/java-monorepo/@refs/heads/main",
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@",
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/*",
        "ChainArgos/java-monorepo/.github/workflows/../release.yml@refs/heads/main",
        "ChainArgos/java-monorepo/.github/workflows/.yml@refs/heads/main",
    ] {
        assert!(
            !valid_group_workflow_identity(repository, workflow),
            "{workflow}"
        );
    }
}
