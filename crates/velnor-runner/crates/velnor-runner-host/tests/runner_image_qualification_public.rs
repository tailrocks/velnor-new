// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "runner_image_qualification_public_extended.rs"]
mod extended;
#[path = "runner_image_qualification_fixture.rs"]
mod fixture;
#[path = "runner_image_qualification_public/support.rs"]
mod support;
#[path = "runner_image_qualification_public/uncertain_cleanup.rs"]
mod uncertain_cleanup;

use std::time::Duration;
use std::{error::Error, fs};

use fixture::{
    ApiFault, ApiPlan, ContainerRole, DockerApiFixture, GitFixture, ImageFault, ImageRole,
    OuterMutation, Probe,
};
use support::*;

type TestResult<T> = Result<T, Box<dyn Error>>;

#[test]
fn public_qualifier_success_keeps_images_and_cleans_private_resources() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.foreign_sentinel = true;
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_success(&output, &state);
    assert!(state.foreign_container_present);
    assert!(state.foreign_volume_present);
    assert_eq!(state.containers.len(), 1);
    assert_eq!(state.volumes, ["foreign-sentinel"]);
    assert_private_configs(&state)?;
    Ok(())
}

#[test]
fn replacement_ref_preserves_source_a_qualification() -> TestResult<()> {
    let git = GitFixture::new()?;
    let source = git.source_sha().to_owned();
    let lock = fs::read(git.root().join(".velnor/runner.lock"))?;
    git.add_replacement_ref(false)?;

    let (output, state) = qualify(&git, &source, ApiPlan::new(&source))?;
    assert_success(&output, &state);
    assert_eq!(fs::read(git.root().join(".velnor/runner.lock"))?, lock);
    Ok(())
}

#[test]
fn replacement_ref_rejects_physical_source_b_before_docker() -> TestResult<()> {
    let git = GitFixture::new()?;
    let source = git.source_sha().to_owned();
    git.add_replacement_ref(true)?;

    let (output, state) = qualify(&git, &source, ApiPlan::new(&source))?;
    assert_source_rejected_before_api(&output, &state);
    Ok(())
}

#[test]
fn public_qualifier_admits_empty_first_readiness_output_and_retries() -> TestResult<()> {
    let git = GitFixture::new()?;
    let (output, state) = qualify(&git, git.source_sha(), ApiPlan::new(git.source_sha()))?;
    assert_success(&output, &state);
    assert!(state.readiness_attempts >= 2);
    assert!(state.empty_readiness_attempts > 0);
    Ok(())
}

#[test]
fn public_qualifier_rejects_invalid_arguments_before_docker() -> TestResult<()> {
    let git = GitFixture::new()?;
    let api = DockerApiFixture::new(ApiPlan::new(git.source_sha()))?;
    let output = fixture::invoke(
        git.root(),
        "not-a-source-sha",
        &digest_id('a'),
        &digest_id('b'),
        api.socket(),
    )?;
    assert_rejected_before_api(&output, &api.snapshot()?);
    Ok(())
}

#[test]
fn public_qualifier_rejects_wrong_head_and_worktree_changes() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.parent_sha());
    let (output, state) = qualify(&git, git.parent_sha(), plan)?;
    assert_source_rejected_before_api(&output, &state);

    let git = GitFixture::new()?;
    git.change_tracked()?;
    plan = ApiPlan::new(git.source_sha());
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_source_rejected_before_api(&output, &state);

    let git = GitFixture::new()?;
    git.add_untracked()?;
    let (output, state) = qualify(&git, git.source_sha(), ApiPlan::new(git.source_sha()))?;
    assert_source_rejected_before_api(&output, &state);
    Ok(())
}

#[test]
fn public_qualifier_rejects_image_identity_and_platform_mismatches() -> TestResult<()> {
    for fault in [
        ImageFault::Id,
        ImageFault::Platform,
        ImageFault::Source,
        ImageFault::Reinspection,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.image_fault = Some((ImageRole::Dind, fault));
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert_failed_and_cleaned(&output, &state);
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_wrong_image_kind_before_resources() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.foreign_sentinel = true;
    plan.image_fault = Some((ImageRole::Dind, ImageFault::Kind));
    let (output, state) = qualify(&git, git.source_sha(), plan)?;

    assert!(
        !output.status.success(),
        "accepted wrong-role image; api_requests={}, image_inspections={}, volume_creations={}, containers={:?}, volumes={:?}",
        state.api_requests,
        state.image_inspections,
        state.volume_creations,
        state.containers,
        state.volumes
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
    assert!(state.fault_reached);
    assert_eq!(state.image_inspections, 2);
    assert_eq!(state.volume_creations, 0);
    assert!(state.created.is_empty());
    assert!(state.foreign_container_present && state.foreign_volume_present);
    assert_eq!(state.containers.len(), 1);
    assert_eq!(state.containers[0].name, "foreign-sentinel");
    assert_eq!(state.volumes, ["foreign-sentinel"]);
    assert_eq!(state.images.len(), 2);
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));
    Ok(())
}

#[test]
fn public_qualifier_rejects_each_locked_image_version_mismatch() -> TestResult<()> {
    for probe in [
        Probe::RunnerListener,
        Probe::RunnerDocker,
        Probe::DindDocker,
        Probe::DindDockerd,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.version_mismatch = Some(probe);
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert_failed_and_cleaned(&output, &state);
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_wrong_daemon_relationships_and_changes() -> TestResult<()> {
    for mutate in [DaemonMutation::SharedId, DaemonMutation::RunnerSocket] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        match mutate {
            DaemonMutation::SharedId => plan.inner_id = plan.outer_id.clone(),
            DaemonMutation::RunnerSocket => plan.fault = Some(ApiFault::WrongRunnerSocketDaemon),
        }
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert_failed_and_cleaned(&output, &state);
    }
    for change in [
        OuterMutation::Id,
        OuterMutation::Root,
        OuterMutation::Version,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.outer_mutation = change;
        plan.foreign_sentinel = true;
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
        assert!(state.fault_reached);
        assert_eq!(state.outer_info_calls, 2);
        assert!(state.foreign_container_present && state.foreign_volume_present);
        assert_eq!(state.containers.len(), 1);
        assert_eq!(state.containers[0].name, "foreign-sentinel");
        assert_eq!(state.volumes, ["foreign-sentinel"]);
        assert_eq!(state.images.len(), 2);
        assert!(state.images.contains(&digest_id('a')));
        assert!(state.images.contains(&digest_id('b')));
    }
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::RunnerSocketFailure);
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_failed_and_cleaned(&output, &state);
    Ok(())
}

#[test]
fn public_qualifier_revalidates_source_and_lock_after_qualification() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.mutate_lock_on_first_image_inspect = Some(git.root().join(".velnor/runner.lock"));
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_failed_and_cleaned(&output, &state);

    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.mutate_source_on_first_image_inspect = Some(git.root().join("tracked.txt"));
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_failed_and_cleaned(&output, &state);
    Ok(())
}

#[test]
fn public_qualifier_cleans_up_when_container_creation_response_is_lost() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::LoseContainerCreateResponse);
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_failed_and_cleaned(&output, &state);
    assert!(!state.created.is_empty());
    Ok(())
}

#[test]
fn public_qualifier_removes_owned_containers_rejected_for_configuration() -> TestResult<()> {
    for role in [ContainerRole::RunnerListener, ContainerRole::Dind] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(ApiFault::WrongOwnedConfig(role));
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert_failed_and_cleaned(&output, &state);
    }
    Ok(())
}

#[test]
fn public_qualifier_distinguishes_absence_from_cleanup_failures() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::CleanupNotFound);
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    assert_success(&output, &state);
    assert!(state.fault_reached);

    for fault in [
        ApiFault::CleanupServerError,
        ApiFault::CleanupDisconnect,
        ApiFault::CleanupStall,
        ApiFault::CleanupStillPresent,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(!output.status.success(), "accepted cleanup fault {fault:?}");
        assert!(state.api_requests > 0);
        assert!(state.fault_reached);
        assert!(
            !state.containers.is_empty(),
            "lost retained owned container"
        );
        if fault == ApiFault::CleanupStillPresent {
            assert_eq!(state.containers.len(), 1);
            assert_eq!(state.volumes.len(), 1);
            let container = &state.containers[0];
            assert_eq!(container.role, "runner-socket-query");
            assert_eq!(
                container.host_config["Mounts"][0]["Source"],
                state.volumes[0]
            );
        }
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_nonzero_malformed_stderr_and_oversized_probe_output() -> TestResult<()>
{
    for fault in [
        ApiFault::NonzeroWait,
        ApiFault::ProbeStderr,
        ApiFault::MalformedOutput,
        ApiFault::OversizedOutput,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert_failed_and_cleaned(&output, &state);
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_unfinished_exec() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::UnfinishedExec);
    let (output, state) =
        qualify_with_timeout(&git, git.source_sha(), plan, Duration::from_secs(24))?;
    assert!(!output.status.success());
    assert!(state.api_requests > 0);
    assert!(state.fault_reached);
    assert!(state.containers.is_empty());
    assert_eq!(state.volumes, Vec::<String>::new());
    Ok(())
}

#[test]
fn public_qualifier_bounds_docker_operations_and_streams() -> TestResult<()> {
    for fault in [
        ApiFault::StallImageInspect,
        ApiFault::StallContainerCreate,
        ApiFault::StallContainerStart,
        ApiFault::StallContainerInspect,
        ApiFault::StallWait,
        ApiFault::StallLogs,
        ApiFault::StallExecCreate,
        ApiFault::StallExecStream,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        let (output, state, elapsed) =
            qualify_with_timeout_timed(&git, git.source_sha(), plan, Duration::from_secs(24))?;
        assert_failed_and_cleaned(&output, &state);
        let readiness = matches!(fault, ApiFault::StallExecCreate | ApiFault::StallExecStream);
        assert!(elapsed < Duration::from_secs(if readiness { 22 } else { 9 }));
        assert!(elapsed >= Duration::from_secs(if readiness { 11 } else { 4 }));
    }
    Ok(())
}
