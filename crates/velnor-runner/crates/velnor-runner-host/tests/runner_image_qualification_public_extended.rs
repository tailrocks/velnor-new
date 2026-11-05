// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::*;

#[test]
fn public_qualifier_rejects_hidden_and_ignored_lock_changes() -> TestResult<()> {
    for index_flag in ["--skip-worktree", "--assume-unchanged"] {
        let git = GitFixture::new()?;
        git.hide_lock_change(index_flag)?;
        let (output, state) = qualify(&git, git.source_sha(), ApiPlan::new(git.source_sha()))?;
        assert_source_rejected_before_api(&output, &state);
    }
    let git = GitFixture::new_with_ignored_lock()?;
    let api = DockerApiFixture::new(ApiPlan::new(git.source_sha()))?;
    let output = fixture::invoke(
        git.root(),
        git.source_sha(),
        &digest_id('a'),
        &digest_id('b'),
        api.socket(),
    )?;
    assert_source_rejected_before_api(&output, &api.snapshot()?);
    Ok(())
}

#[test]
fn public_qualifier_rejects_hidden_tracked_changes_before_docker() -> TestResult<()> {
    for hide_change in [
        GitFixture::hide_tracked_skip_worktree_change,
        GitFixture::hide_tracked_assume_unchanged_change,
    ] {
        let git = GitFixture::new()?;
        hide_change(&git)?;
        assert!(git.diff_head_is_clean()?);
        let (output, state) = qualify(&git, git.source_sha(), ApiPlan::new(git.source_sha()))?;
        assert_source_rejected_before_api(&output, &state);
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_invalid_image_ids_and_socket_arguments() -> TestResult<()> {
    let git = GitFixture::new()?;
    let api = DockerApiFixture::new(ApiPlan::new(git.source_sha()))?;
    let valid_socket = api.socket().to_string_lossy().into_owned();
    let cases = [
        ("sha256:zz".to_owned(), digest_id('b'), valid_socket.clone()),
        (
            digest_id('a'),
            "sha256:bad".to_owned(),
            valid_socket.clone(),
        ),
        (digest_id('a'), digest_id('a'), valid_socket),
        (digest_id('a'), digest_id('b'), "relative.sock".to_owned()),
        (
            digest_id('a'),
            digest_id('b'),
            "tcp://localhost:2375".to_owned(),
        ),
    ];
    for (runner, dind, socket) in cases {
        let output = fixture::invoke(
            git.root(),
            git.source_sha(),
            &runner,
            &dind,
            Path::new(&socket),
        )?;
        assert_rejected_before_api(&output, &api.snapshot()?);
    }
    Ok(())
}

#[test]
fn public_qualifier_preserves_foreign_owner_and_replaced_ids() -> TestResult<()> {
    for fault in [
        ApiFault::ForeignOwnerAtTrackedName,
        ApiFault::ReplacedContainerId,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(!output.status.success());
        assert!(
            state.api_requests > 0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(state.fault_reached);
        assert_eq!(state.containers.len(), 1);
        assert_eq!(state.volumes, Vec::<String>::new());
        let container = &state.containers[0];
        assert!(
            container
                .name
                .starts_with("velnor-qualify-runner-listener-")
        );
        assert_eq!(container.role, "runner-listener");
        if fault == ApiFault::ForeignOwnerAtTrackedName {
            assert_eq!(
                container.labels["io.velnor.runner-image-qualifier.run"],
                "foreign-run"
            );
            assert_eq!(
                container.labels["io.velnor.runner-image-qualifier.role"],
                "foreign"
            );
        } else {
            assert_eq!(container.id, "e".repeat(64));
            assert_eq!(
                container.labels["io.velnor.runner-image-qualifier.role"],
                "runner-listener"
            );
        }
    }
    Ok(())
}

#[test]
fn public_qualifier_recovers_volume_created_before_lost_response() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::LoseVolumeCreateResponse);
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_failed_and_cleaned(&output, &state);
    assert!(state.fault_reached);
    assert!(state.volume_creations > 0);
    Ok(())
}

#[test]
fn public_qualifier_accepts_only_absent_volume_cleanup_as_success() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::VolumeCleanupNotFound);
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_success(&output, &state);
    assert!(state.fault_reached);

    for fault in [
        ApiFault::VolumeCleanupServerError,
        ApiFault::VolumeCleanupDisconnect,
        ApiFault::VolumeCleanupStall,
        ApiFault::VolumeCleanupStillPresent,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(
            !output.status.success(),
            "accepted volume cleanup {fault:?}"
        );
        assert!(
            state.api_requests > 0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(state.fault_reached);
        assert!(state.containers.is_empty());
        assert!(state.images.contains(&digest_id('a')));
        assert!(state.images.contains(&digest_id('b')));
        if fault != ApiFault::VolumeCleanupStall {
            assert_eq!(state.volumes.len(), 1);
        }
    }
    Ok(())
}

#[test]
fn public_qualifier_recovers_late_container_and_volume_creates() -> TestResult<()> {
    for fault in [ApiFault::LateContainerCreate, ApiFault::LateVolumeCreate] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        plan.foreign_sentinel = true;
        let (output, api, elapsed) =
            invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(20))?;
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
        assert!(elapsed >= Duration::from_secs(4));
        assert!(elapsed < Duration::from_secs(14));
        let state = api.settled_snapshot(Duration::from_secs(10))?;
        assert!(state.fault_reached);
        assert_eq!(state.pending_mutations, 0);
        assert_eq!(state.containers.len(), 1);
        assert_eq!(state.containers[0].name, "foreign-sentinel");
        assert_eq!(state.volumes, ["foreign-sentinel"]);
        assert_eq!(state.images.len(), 2);
        assert!(state.images.contains(&digest_id('a')));
        assert!(state.images.contains(&digest_id('b')));
        if fault == ApiFault::LateContainerCreate {
            assert_eq!(state.created.len(), 1);
            assert_eq!(state.created[0].role, "runner-listener");
            assert_eq!(state.readiness_attempts, 0);
        } else {
            assert_eq!(state.volume_creations, 1);
        }
    }
    Ok(())
}

#[test]
fn public_qualifier_reconciles_disconnect_before_late_create_insertion() -> TestResult<()> {
    for fault in [
        ApiFault::DisconnectThenLateContainerCreate,
        ApiFault::DisconnectThenLateVolumeCreate,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        plan.foreign_sentinel = true;
        let (output, api, elapsed) =
            invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(20))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
        assert!(elapsed < Duration::from_secs(18));
        let state = api.settled_snapshot(Duration::from_secs(8))?;
        assert!(state.fault_reached);
        assert_eq!(state.pending_mutations, 0);
        let owned_containers = state
            .containers
            .iter()
            .filter(|container| container.name != "foreign-sentinel")
            .collect::<Vec<_>>();
        let owned_volumes = state
            .volumes
            .iter()
            .filter(|volume| *volume != "foreign-sentinel")
            .collect::<Vec<_>>();
        if !owned_containers.is_empty() || !owned_volumes.is_empty() {
            if let Some(container) = owned_containers.first() {
                assert!(stderr.contains("container create state is uncertain"));
                assert!(
                    stderr.contains(&format!("recovery identity: container {}", container.name))
                );
            }
            if let Some(volume) = owned_volumes.first() {
                assert!(stderr.contains("private-volume create state is uncertain"));
                assert!(stderr.contains(&format!("recovery identity: volume {volume}")));
            }
        }
        assert_eq!(state.containers.len(), 1 + owned_containers.len());
        assert!(state.containers.iter().any(|container| {
            container.name == "foreign-sentinel"
                && container.labels["io.velnor.runner-image-qualifier.run"] == "foreign-run"
        }));
        assert_eq!(state.volumes.len(), 1 + owned_volumes.len());
        assert!(
            state
                .volumes
                .iter()
                .any(|volume| volume == "foreign-sentinel")
        );
        assert_eq!(state.images.len(), 2);
        assert!(state.images.contains(&digest_id('a')));
        assert!(state.images.contains(&digest_id('b')));
    }
    Ok(())
}

#[test]
fn public_qualifier_reports_unsettled_create_and_preserves_replacement() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::NeverSettleContainerCreate);
    plan.foreign_sentinel = true;
    let (output, api, elapsed) =
        invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(20))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
    assert!(stderr.contains("container create state is uncertain"));
    assert!(stderr.contains("recovery identity: container velnor-qualify-runner-listener-"));
    assert!(elapsed >= Duration::from_secs(9));
    assert!(elapsed < Duration::from_secs(16));
    let state = api.snapshot()?;
    assert!(state.fault_reached);
    assert_eq!(state.pending_mutations, 1);
    let identity = state
        .pending_identities
        .first()
        .ok_or("pending recovery identity is missing")?;
    assert!(stderr.contains(&format!("recovery identity: {identity}")));
    assert_eq!(state.containers.len(), 1);
    assert_eq!(state.containers[0].name, "foreign-sentinel");
    assert_eq!(state.volumes, ["foreign-sentinel"]);
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));

    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::LateContainerCreateReplacement);
    plan.foreign_sentinel = true;
    let (output, api, _) = invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(20))?;
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
    let state = api.settled_snapshot(Duration::from_secs(10))?;
    assert!(state.fault_reached);
    assert_eq!(state.pending_mutations, 0);
    assert_eq!(state.containers.len(), 2);
    assert_eq!(state.volumes, ["foreign-sentinel"]);
    let replacement = state
        .containers
        .iter()
        .find(|container| {
            container
                .name
                .starts_with("velnor-qualify-runner-listener-")
        })
        .ok_or("replacement container is missing")?;
    assert_eq!(replacement.id, "e".repeat(64));
    assert_eq!(
        replacement.labels["io.velnor.runner-image-qualifier.run"],
        "foreign-run"
    );
    assert_eq!(
        replacement.labels["io.velnor.runner-image-qualifier.role"],
        "foreign"
    );
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));
    Ok(())
}

#[test]
fn public_qualifier_rejects_hidden_tracked_changes_during_inspection() -> TestResult<()> {
    for mutate in [
        ApiPlan::mutate_skip_worktree_on_first_image_inspect,
        ApiPlan::mutate_assume_unchanged_on_first_image_inspect,
    ] {
        let git = GitFixture::new()?;
        assert!(git.diff_head_is_clean()?);
        let mut plan = ApiPlan::new(git.source_sha());
        plan.foreign_sentinel = true;
        mutate(&mut plan, git.root().to_path_buf());
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(stderr.contains("worker identity"), "{stderr}");
        assert!(!stdout.contains("qualified_runner_image_id"));
        assert!(matches!(state.hidden_index_mutation, Some(Ok(()))));
        assert!(git.diff_head_is_clean()?);
        assert!(state.api_requests > 0);
        assert!(state.image_inspections > 0);
        assert!(state.foreign_container_present && state.foreign_volume_present);
        assert_eq!(state.containers.len(), 1);
        assert_eq!(state.containers[0].name, "foreign-sentinel");
        assert_eq!(state.volumes, ["foreign-sentinel"]);
        assert_eq!(state.images.len(), 2);
        assert!(state.images.contains(&digest_id('a')));
        assert!(state.images.contains(&digest_id('b')));
    }
    Ok(())
}
