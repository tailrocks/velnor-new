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
