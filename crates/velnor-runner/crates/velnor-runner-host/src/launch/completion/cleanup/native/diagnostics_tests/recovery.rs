use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, process::Command};

use super::*;

#[path = "recovery/capture_limits.rs"]
mod capture_limits;
#[path = "recovery/lifecycle.rs"]
mod lifecycle;
#[path = "recovery/ownership_retry.rs"]
mod ownership_retry;
#[path = "recovery/readiness_cleanup.rs"]
mod readiness_cleanup;

#[test]
fn missing_runner_trace_is_persisted_as_unavailable_and_blocks_deletion() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "missing_runner_trace_is_persisted_as_unavailable_and_blocks_deletion",
        || {
            Box::pin(async {
                Box::pin(missing_runner_trace_blocks_native_deletion_on_fixture_stack())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn missing_runner_trace_blocks_native_deletion_on_fixture_stack() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    fixture.server.expose_empty_runner_logs().await;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let root = diagnostics_root(&fixture)?;
    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::not_proven(
            "runner logs unavailable"
        ))
    );
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    let state_bytes = std::fs::read(runner_log_state_path(&fixture, launch.intent.id)?)
        .map_err(|_| HostError::Path)?;
    let state: serde_json::Value =
        serde_json::from_slice(&state_bytes).map_err(|_| HostError::Frame)?;
    if state["phase"] != "unavailable" || state["has_trace"] != false {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[test]
fn reopened_partial_native_capture_keeps_containers_and_volumes() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "reopened_partial_native_capture_keeps_containers_and_volumes",
        || {
            Box::pin(async {
                Box::pin(
                    reopened_partial_native_capture_keeps_containers_and_volumes_on_fixture_stack(),
                )
                .await
                .map_err(|error| error.to_string())
            })
        },
    )
}

async fn reopened_partial_native_capture_keeps_containers_and_volumes_on_fixture_stack()
-> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let root = diagnostics_root(&fixture)?;
    drop(root);
    let path = diagnostic_path(
        &fixture,
        launch.intent.id,
        claim,
        "runner",
        launch
            .intent
            .docker_id
            .as_deref()
            .ok_or(HostError::Ownership)?,
    )?;
    std::fs::write(&path, b"{\"version\":1").map_err(|_| HostError::Path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Path)?;

    let root = diagnostics_root(&fixture)?;
    let before = std::fs::read(&path).map_err(|_| HostError::Path)?;
    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::request(
            "diagnostic write",
            HostError::Ownership,
        ))
    );
    assert_eq!(std::fs::read(path).map_err(|_| HostError::Path)?, before);
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    Ok(())
}

#[test]
fn diagnostics_worker_pending_before_reconcile_blocks_native_deletion() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "diagnostics_worker_pending_before_reconcile_blocks_native_deletion",
        || {
            Box::pin(async {
                Box::pin(
                    diagnostics_worker_pending_before_reconcile_blocks_native_deletion_on_fixture_stack(),
                )
                .await
                .map_err(|error| error.to_string())
            })
        },
    )
}

async fn diagnostics_worker_pending_before_reconcile_blocks_native_deletion_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let root = diagnostics_root(&fixture)?;
    let pending = root.hold_pending_write_for_test(launch.intent.id, claim.generation)?;

    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::not_proven(
            "diagnostic write pending",
        ))
    );
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    pending.finish().await?;
    Ok(())
}

#[test]
fn crashed_child_partial_native_record_keeps_containers_and_volumes() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "crashed_child_partial_native_record_keeps_containers_and_volumes",
        || {
            Box::pin(async {
                crashed_child_partial_native_record_case()
                    .await
                    .map_err(|e| e.to_string())
            })
        },
    )
}

async fn crashed_child_partial_native_record_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let container_id = launch
        .intent
        .docker_id
        .as_deref()
        .ok_or(HostError::Ownership)?;
    let path = diagnostic_path(&fixture, launch.intent.id, claim, "runner", container_id)?;
    drop(diagnostics_root(&fixture)?);

    let output = Command::new(std::env::current_exe().map_err(|_| HostError::Path)?)
        .args([
            "--exact",
            "launch::completion::cleanup::native::diagnostics_tests::recovery::native_capture_crash_child_writer",
            "--nocapture",
        ])
        .env("VELNOR_TEST_NATIVE_CAPTURE_CRASH_PATH", &path)
        .output()
        .map_err(|_| HostError::Path)?;
    if output.status.code() != Some(23) {
        return Err(HostError::Ownership);
    }

    let root = diagnostics_root(&fixture)?;
    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::request(
            "diagnostic write",
            HostError::Ownership
        ))
    );
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    Ok(())
}

#[test]
fn native_capture_crash_child_writer() {
    let Ok(path) = std::env::var("VELNOR_TEST_NATIVE_CAPTURE_CRASH_PATH") else {
        return;
    };
    let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    else {
        std::process::exit(24);
    };
    if file.write_all(b"{\"schema\":1").is_err() || file.sync_all().is_err() {
        std::process::exit(25);
    }
    std::process::exit(23);
}

#[test]
fn native_home_volume_delete_failure_keeps_capacity_charged() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "native_home_volume_delete_failure_keeps_capacity_charged",
        || {
            Box::pin(async {
                native_home_volume_delete_failure_case()
                    .await
                    .map_err(|e| e.to_string())
            })
        },
    )
}

async fn native_home_volume_delete_failure_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    fixture
        .server
        .fail_owned_volume_removal(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let root = super::diagnostics_root(&fixture)?;
    let result = Box::pin(super::run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;

    assert!(matches!(
        result,
        Err(crate::launch::completion::cleanup::Failure {
            phase: "volume remove",
            ..
        })
    ));
    assert_eq!(fixture.server.effects().container_deletes, 2);
    assert_eq!(fixture.server.effects().volume_deletes, 2);
    let crate::launch::bootstrap::CleanupRuntimeFixture { base, .. } = &fixture;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        reservation_assignment,
        ..
    } = base;
    let reservation = fixture
        .scope
        .reservation(reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert!(
        !fixture
            .scope
            .native_completion_reservation(&launch)
            .await?
            .ok_or(HostError::Journal)?
            .storage_cleanup_proven()
    );
    Ok(())
}

#[test]
fn native_home_volume_post_delete_inspection_failure_keeps_capacity_charged() -> Result<(), String>
{
    run_serialized_diagnostic_test(
        "native_home_volume_post_delete_inspection_failure_keeps_capacity_charged",
        || {
            Box::pin(async {
                Box::pin(native_home_volume_post_delete_inspection_failure_case())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_home_volume_post_delete_inspection_failure_case() -> Result<(), HostError> {
    Box::pin(ownership_retry::native_home_volume_post_delete_inspection_failure_case()).await
}
