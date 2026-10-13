use super::*;

#[test]
fn organization_preflight_keeps_ubuntu24_profile_out_of_admission_without_io() {
    let (mut transport, intents) = paired();
    let workflows =
        ["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main".to_owned()];
    let expected = PoolBindingView {
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
        runner_image: Some(test_runner_image()),
        allowed_group_workflows: &workflows,
        policy_digest: "policy-digest",
    };

    let preflight = block_on_ready(preflight_organization_pool_admission_with_admin_async(
        &mut transport,
        expected,
        "hostcredential",
        "hostcredential",
        &mut intents.clone(),
    ))
    .expect("profile gate is a typed unknown");
    assert_eq!(
        preflight.evidence,
        PoolAdmissionEvidence::Unknown(
            velnor_runner_github::policy::PolicyGap::RequiredRunnerProfileUnavailable
        )
    );
    assert!(preflight.verified_admin.is_none());
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        0
    );
    let state = intents.0.lock().expect("test intent lock");
    assert_eq!(state.rows.len(), 0);
    assert_eq!(state.events.len(), 0);
}

#[test]
fn repository_scoped_preflight_stops_before_any_request_or_intent() {
    let (mut transport, intents) = paired();
    let workflows = ["ChainArgos/java-monorepo/.github/workflows/ci.yml@main".to_owned()];
    let expected = PoolBindingView {
        registration_scope: PoolRegistrationScopeView::Repository {
            owner: "ChainArgos",
            repository: "java-monorepo",
        },
        target_repository_id: None,
        target_repository_full_name: "ChainArgos/java-monorepo",
        scale_set_id: None,
        scale_set_name: "ubuntu-24.04-scale-set",
        actions_runner_group_id: 1,
        actions_runner_group_name: "Default",
        rest_runner_group_id: None,
        runner_image: None,
        allowed_group_workflows: &workflows,
        policy_digest: "policy-digest",
    };
    let preflight = block_on_ready(preflight_pool_admission_with_admin_async(
        &mut transport,
        expected,
        "hostcredential",
        "hostcredential",
        &mut intents.clone(),
    ))
    .expect("repo scope remains a non-error unknown");
    assert_eq!(
        preflight.evidence,
        PoolAdmissionEvidence::Unknown(
            velnor_runner_github::policy::PolicyGap::EffectiveRoutingApplicabilityUnproven
        )
    );
    assert!(preflight.verified_admin.is_none());
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .as_slice(),
        &[]
    );
    assert_eq!(
        intents.0.lock().expect("test intent lock").rows.as_slice(),
        &[]
    );
}

#[test]
fn organization_preflight_without_exact_group_workflow_rules_is_unknown_without_io() {
    let (mut transport, intents) = paired();
    let expected = PoolBindingView {
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
        allowed_group_workflows: &[],
        policy_digest: "policy-digest",
    };
    assert_eq!(
        block_on_ready(preflight_organization_pool_admission_async(
            &mut transport,
            expected,
            "hostcredential",
            "hostcredential",
            &mut intents.clone(),
        ))
        .expect("missing exact workflow config remains a typed unknown"),
        PoolAdmissionEvidence::Unknown(
            velnor_runner_github::policy::PolicyGap::WorkflowRuleSetEmpty
        )
    );
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        0
    );
    assert_eq!(intents.0.lock().expect("test intent lock").rows.len(), 0);
}

fn test_runner_image() -> RunnerImageIdentityView<'static> {
    RunnerImageIdentityView {
        profile: "ubuntu-24.04-amd64",
        scale_set_name: "ubuntu-24.04-scale-set",
        platform: "linux/amd64",
        runner_image: "ghcr.io/actions/actions-runner@sha256:660f7b9d1e0007274f7c867e220b3c382137ef5a30d8e501a025ad50dbd5fb9d",
        runner_manifest_digest: "sha256:660f7b9d1e0007274f7c867e220b3c382137ef5a30d8e501a025ad50dbd5fb9d",
        runner_index_digest: "sha256:4ffadc0002b2581327e06101fc8c06cd189232baf79fe561fac9caeb76f5e807",
        runner_config_digest: "sha256:3027ff446b2083fc96c214916922b117f4d996b0683295ce7b43fd0048eae210",
        runner_os: "ubuntu24",
        runner_release_version: "2.338.0",
        runner_release_published_at: "2026-10-06T13:55:11Z",
        // Test fixture deadline is deliberately far in the future. The production
        // resolver supplies its real qualification deadline, which is separately
        // covered by the fixed-clock unit test.
        runner_requalify_by: "2099-11-05T13:55:11Z",
        dind_image: "docker.io/library/docker@sha256:dcac6f16dc25ddec91e2d467605775b95a035ab884b94cb4c2cc7cbef6fd726d",
        dind_manifest_digest: "sha256:dcac6f16dc25ddec91e2d467605775b95a035ab884b94cb4c2cc7cbef6fd726d",
        dind_index_digest: "sha256:7dcdfc4a20246236f558175182ccace1eb15a41bd3eb119dd2284f393498b7c1",
        dind_config_digest: "sha256:a32a1e3b62afa34576d728f5b65075e89e468ebabc56696df28fa256e8ac0b5d",
        dind_version: "29.8.2",
        dind_source: "docker-library/official-images@a888e7fd9fd891fe0d6050620b2b9fff5024c7e0;docker-library/docker@d576eb69d7bad654b934176e95644995aa85d8f8:29/dind",
        dind_entrypoint_sha256: "acf43f8eb1181afbada127661c7d85ebbcf9e3b556d55c314991d2d21c25292a",
    }
}
