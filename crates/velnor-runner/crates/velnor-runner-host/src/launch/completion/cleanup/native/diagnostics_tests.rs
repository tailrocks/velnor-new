use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::{
    error::HostError,
    launch::{bootstrap::CleanupRuntimeFixture, completion},
};

#[path = "diagnostics_tests/recovery.rs"]
mod recovery;

use super::tests::{
    capacity_retry_fixture, diagnostics_root, run_native_cleanup,
    run_native_cleanup_with_diagnostics, seed_completion,
};

#[test]
fn native_cleanup_writes_sanitized_diagnostics_for_exact_owned_containers() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "native_cleanup_writes_sanitized_diagnostics_for_exact_owned_containers",
        || {
            Box::pin(async {
                Box::pin(native_cleanup_writes_sanitized_diagnostics_for_exact_owned_containers_on_fixture_stack()).await.map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_cleanup_writes_sanitized_diagnostics_for_exact_owned_containers_on_fixture_stack()
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

    let cleanup = Box::pin(run_native_cleanup(&fixture, &launch, claim)).await?;
    assert!(
        matches!(
            cleanup,
            Err(completion::cleanup::Failure {
                phase: "metadata capacity hold",
                ..
            })
        ),
        "Native cleanup returned {cleanup:?}. The effects are {:?}.",
        fixture.server.effects()
    );
    assert_eq!(fixture.server.effects().container_deletes, 2);

    let state_bytes = std::fs::read(runner_log_state_path(&fixture, launch.intent.id)?)
        .map_err(|_| HostError::Path)?;
    let state: serde_json::Value =
        serde_json::from_slice(&state_bytes).map_err(|_| HostError::Frame)?;
    if state["phase"] != "complete"
        || state["binding"]["reservation_id"] != fixture.reservation_id
        || state["binding"]["intent_id"] != launch.intent.id
    {
        return Err(HostError::Ownership);
    }
    let projection = std::fs::read(runner_log_projection_path(&fixture, launch.intent.id, 1)?)
        .map_err(|_| HostError::Path)?;
    let projection: serde_json::Value =
        serde_json::from_slice(&projection).map_err(|_| HostError::Frame)?;
    if projection["runner_log_present"] != true
        || projection.to_string().contains("RUNNER_DIAGNOSTIC_CANARY")
    {
        return Err(HostError::Ownership);
    }

    for (role, id) in [
        ("runner", launch.intent.docker_id.as_deref()),
        ("dind", launch.intent.dind_id.as_deref()),
    ] {
        let id = id.ok_or(HostError::Ownership)?;
        let bytes = std::fs::read(diagnostic_path(
            &fixture,
            launch.intent.id,
            claim,
            role,
            id,
        )?)
        .map_err(|_| HostError::Path)?;
        let record: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| HostError::Frame)?;
        if record["role"] != role
            || record["container_id"] != id
            || record["connection_id"] != fixture.scope.connection_id().as_str()
            || bytes
                .windows(b"RUNNER_DIAGNOSTIC_CANARY".len())
                .any(|window| window == b"RUNNER_DIAGNOSTIC_CANARY")
            || record.get("env").is_some()
            || record.get("config").is_some()
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

#[test]
fn native_container_scope_mismatch_prevents_capture_and_deletion() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "native_container_scope_mismatch_prevents_capture_and_deletion",
        || {
            Box::pin(async {
                Box::pin(
                    native_container_scope_mismatch_prevents_capture_and_deletion_on_fixture_stack(
                    ),
                )
                .await
                .map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_container_scope_mismatch_prevents_capture_and_deletion_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers_with_mismatch(
            fixture.scope.connection_id().as_str(),
            crate::launch::bootstrap::OwnedContainerMismatch::Scope,
        )
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;

    let result = Box::pin(run_native_cleanup(&fixture, &launch, claim)).await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::not_proven(
            "container ownership"
        ))
    );
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    Ok(())
}

#[test]
fn native_foreign_container_identity_and_binding_labels_prevent_capture_and_deletion()
-> Result<(), String> {
    run_serialized_diagnostic_test(
        "native_foreign_container_identity_and_binding_labels_prevent_capture_and_deletion",
        || {
            Box::pin(async {
                Box::pin(native_foreign_container_identity_and_binding_labels_prevent_capture_and_deletion_on_fixture_stack())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_foreign_container_identity_and_binding_labels_prevent_capture_and_deletion_on_fixture_stack()
-> Result<(), HostError> {
    use crate::launch::bootstrap::OwnedContainerMismatch as Mismatch;

    for mismatch in [
        Mismatch::Id,
        Mismatch::Name,
        Mismatch::Reservation,
        Mismatch::Operation,
    ] {
        let fixture = Box::pin(capacity_retry_fixture()).await?;
        fixture
            .server
            .expose_owned_containers_with_mismatch(fixture.scope.connection_id().as_str(), mismatch)
            .await?;
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
                "container ownership"
            ))
        );
        assert_eq!(fixture.server.effects().container_deletes, 0);
        assert_eq!(fixture.server.effects().volume_deletes, 0);
        let runner = launch
            .intent
            .docker_id
            .as_deref()
            .ok_or(HostError::Ownership)?;
        assert!(!diagnostic_path(&fixture, launch.intent.id, claim, "runner", runner)?.exists());
    }
    Ok(())
}

#[test]
fn claim_loss_during_native_diagnostic_write_prevents_delete() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "claim_loss_during_native_diagnostic_write_prevents_delete",
        || {
            Box::pin(async {
                Box::pin(
                    claim_loss_during_native_diagnostic_write_prevents_delete_on_fixture_stack(),
                )
                .await
                .map_err(|error| error.to_string())
            })
        },
    )
}

async fn claim_loss_during_native_diagnostic_write_prevents_delete_on_fixture_stack()
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
    let gate = root.pause_next_write_for_test()?;
    let cleanup = run_native_cleanup_with_diagnostics(&fixture, &launch, claim, &root);
    tokio::pin!(cleanup);
    let pending = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if root.pending_for(launch.intent.id)? {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<(), HostError>(())
    });
    tokio::pin!(pending);
    tokio::select! {
        result = &mut cleanup => {
            return Err(result.err().unwrap_or(HostError::Ownership));
        }
        result = &mut pending => {
            result.map_err(|_| HostError::DockerTimeout)??;
        }
    }
    let replacement = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, 4_000_000_000, 90)
        .await?
        .ok_or(HostError::Journal)?;
    if replacement.generation == claim.generation {
        return Err(HostError::Ownership);
    }
    gate.wait();
    assert_eq!(
        cleanup.await?,
        Err(completion::cleanup::Failure::claim_lost())
    );
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    Ok(())
}

fn diagnostic_path(
    fixture: &CleanupRuntimeFixture,
    intent_id: i64,
    claim: crate::journal::CleanupClaim,
    role: &str,
    container_id: &str,
) -> Result<PathBuf, HostError> {
    let crate::launch::bootstrap::CleanupRuntimeFixture { _base: base, .. } = fixture;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch: scratch,
        ..
    } = base;
    let scratch = scratch.file();
    let state = std::fs::canonicalize(scratch.parent().ok_or(HostError::Path)?)
        .map_err(|_| HostError::Path)?;
    let connection =
        completion::diagnostics::scope_component(fixture.scope.connection_id().as_str());
    let digest = Sha256::digest(container_id.as_bytes());
    let filename = format!(
        "i{intent_id}-g{}-{role}-{}.json",
        claim.generation,
        lower_hex(&digest)
    );
    Ok(state
        .join(completion::diagnostics::SCOPE_DIRECTORY)
        .join(connection)
        .join(filename))
}

fn runner_log_state_path(
    fixture: &CleanupRuntimeFixture,
    intent_id: i64,
) -> Result<PathBuf, HostError> {
    let crate::launch::bootstrap::CleanupRuntimeFixture { _base: base, .. } = fixture;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch: scratch,
        ..
    } = base;
    let state = std::fs::canonicalize(scratch.file().parent().ok_or(HostError::Path)?)
        .map_err(|_| HostError::Path)?;
    Ok(state
        .join(completion::diagnostics::SCOPE_DIRECTORY)
        .join(completion::diagnostics::scope_component(
            fixture.scope.connection_id().as_str(),
        ))
        .join(format!(
            "i{intent_id}-r{}-runner-log-state.json",
            fixture.reservation_id
        )))
}

fn runner_log_projection_path(
    fixture: &CleanupRuntimeFixture,
    intent_id: i64,
    attempt: i64,
) -> Result<PathBuf, HostError> {
    let state_path = runner_log_state_path(fixture, intent_id)?;
    let parent = state_path.parent().ok_or(HostError::Path)?;
    Ok(parent.join(format!("i{intent_id}-g{attempt}-runner-logs.json")))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

fn run_serialized_diagnostic_test<F>(name: &'static str, future_factory: F) -> Result<(), String>
where
    F: FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>
        + Send
        + 'static,
{
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().map_err(|error| error.to_string())?;
    crate::launch::test_harness_stack::run_on_test_harness_stack(name, future_factory)
}
