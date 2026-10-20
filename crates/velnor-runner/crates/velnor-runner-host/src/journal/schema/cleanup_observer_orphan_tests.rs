//! Cleanup observer orphan-row tests.

use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::NativePreChallengeFixture};

#[tokio::test]
async fn orphan_cleanup_observer_operation_blocks_journal_reopen() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    let NativePreChallengeFixture {
        _scratch,
        _journal,
        database_path,
        scope,
        ..
    } = fixture;
    let connection = _journal.connection().await?;
    let request_sha = "a".repeat(64);
    connection
        .execute(
            "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'aborted', NULL, 'resource-probe', 'cleanup-observer', ?10, ?11)",
            ("f".repeat(32), "a".repeat(32), "engine", "b".repeat(64), "c".repeat(40), "runtime-image", "d".repeat(64), "orphan-cleanup-observer", "e".repeat(64), request_sha, "{}"),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(scope);
    drop(_journal);

    assert!(matches!(
        Journal::open(&database_path).await,
        Err(HostError::Journal)
    ));
    drop(_scratch);
    Ok(())
}
