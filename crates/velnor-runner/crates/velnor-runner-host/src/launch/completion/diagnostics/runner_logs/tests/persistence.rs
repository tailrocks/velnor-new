use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

use crate::error::HostError;
use crate::launch::completion::diagnostics::stage::OperationKey;
use crate::launch::completion::diagnostics::{DiagnosticsRoot, scope_component};
use sha2::{Digest, Sha256};

use super::super::{LOG_PATH, RunnerLogStageRequest};
use super::common::valid_archive;

#[test]
fn request_persists_only_private_projection_and_replays_exactly() -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = std::env::temp_dir().join(format!(
        "velnor-runner-log-test-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&state).map_err(|error| error.to_string())?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let state = fs::canonicalize(state).map_err(|error| error.to_string())?;
    let root =
        DiagnosticsRoot::open(&state, "test-connection").map_err(|error| format!("{error:?}"))?;
    let key = OperationKey {
        journal_identity: root.journal_identity.clone(),
        connection_id: root.connection_id.clone(),
        intent_id: 17,
    };
    let container_id = "a".repeat(64);
    let archive = valid_archive()?;
    let started = Instant::now();
    let request =
        RunnerLogStageRequest::new(&root, key.clone(), 4, &container_id, &archive, started)
            .map_err(|error| format!("{error:?}"))?;
    if request.key() != &key {
        return Err("worker request changed operation ownership".to_owned());
    }
    let (expected_digest, expected_size) = request.projection_identity();
    request
        .write_request(&AtomicBool::new(false))
        .map_err(|error| format!("{error:?}"))?;
    let path = state
        .join("completion-diagnostics")
        .join(scope_component("test-connection"))
        .join("i17-g4-runner-logs.json");
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err("runner log projection is not mode 0600".to_owned());
    }
    let written = fs::read(&path).map_err(|error| error.to_string())?;
    let actual_digest: [u8; 32] = Sha256::digest(&written).into();
    if expected_size != written.len() || expected_digest != actual_digest {
        return Err("projection identity did not match the exact staged bytes".to_owned());
    }
    if written
        .windows(b"canary-message-9e26".len())
        .any(|window| window == b"canary-message-9e26")
    {
        return Err("raw log message was written to the private store".to_owned());
    }
    let replay = RunnerLogStageRequest::new(&root, key, 4, &container_id, &archive, Instant::now())
        .map_err(|error| format!("{error:?}"))?;
    replay
        .write_request(&AtomicBool::new(false))
        .map_err(|error| format!("{error:?}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .map_err(|error| error.to_string())?;
    let loose_mode_request = RunnerLogStageRequest::new(
        &root,
        OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: 17,
        },
        4,
        &container_id,
        &archive,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    if loose_mode_request.write_request(&AtomicBool::new(false)) != Err(HostError::Path) {
        return Err("existing projection with a loose mode was accepted".to_owned());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    assert_partial_and_symlink_outputs_are_rejected(&root, &state, &path, &container_id, &archive)?;
    drop(root);
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

fn assert_partial_and_symlink_outputs_are_rejected(
    root: &DiagnosticsRoot,
    state: &std::path::Path,
    path: &std::path::Path,
    container_id: &str,
    archive: &[u8],
) -> Result<(), String> {
    fs::write(path, b"partial or altered record").map_err(|error| error.to_string())?;
    let corrupt_replay = RunnerLogStageRequest::new(
        root,
        OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: 17,
        },
        4,
        container_id,
        archive,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    if corrupt_replay.write_request(&AtomicBool::new(false)) != Err(HostError::Ownership)
        || fs::read(path).map_err(|error| error.to_string())? != b"partial or altered record"
    {
        return Err("partial existing output was replaced or accepted".to_owned());
    }
    let target = state.join("outside-target");
    fs::write(&target, b"private canary target").map_err(|error| error.to_string())?;
    let symlink_path = path.with_file_name("i18-g4-runner-logs.json");
    std::os::unix::fs::symlink(&target, &symlink_path).map_err(|error| error.to_string())?;
    let symlink_request = RunnerLogStageRequest::new(
        root,
        OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: 18,
        },
        4,
        container_id,
        archive,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    if symlink_request
        .write_request(&AtomicBool::new(false))
        .is_ok()
        || fs::read(&target).map_err(|error| error.to_string())? != b"private canary target"
    {
        return Err("symlink output target was followed or modified".to_owned());
    }
    let directory_request = RunnerLogStageRequest::new(
        root,
        OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: 20,
        },
        4,
        container_id,
        archive,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let directory_path = path.with_file_name("i20-g4-runner-logs.json");
    fs::create_dir(&directory_path).map_err(|error| error.to_string())?;
    if directory_request.write_request(&AtomicBool::new(false)) != Err(HostError::Path) {
        return Err("directory at the output path was accepted as a file".to_owned());
    }
    Ok(())
}

#[test]
fn cancelled_write_creates_no_projection_file() -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = std::env::temp_dir().join(format!(
        "velnor-runner-log-cancel-test-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&state).map_err(|error| error.to_string())?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let state = fs::canonicalize(state).map_err(|error| error.to_string())?;
    let root =
        DiagnosticsRoot::open(&state, "test-connection").map_err(|error| format!("{error:?}"))?;
    let request = RunnerLogStageRequest::new(
        &root,
        OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: 19,
        },
        4,
        &"a".repeat(64),
        &valid_archive()?,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    if request.write_request(&AtomicBool::new(true)) != Err(HostError::DockerTimeout) {
        return Err("cancelled write did not fail closed".to_owned());
    }
    let path = state
        .join("completion-diagnostics")
        .join(scope_component("test-connection"))
        .join("i19-g4-runner-logs.json");
    if path.exists() {
        return Err("cancelled write left a projection file".to_owned());
    }
    drop(root);
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn request_rejects_an_operation_key_from_another_connection() -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = std::env::temp_dir().join(format!(
        "velnor-runner-log-key-test-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&state).map_err(|error| error.to_string())?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let state = fs::canonicalize(state).map_err(|error| error.to_string())?;
    let root =
        DiagnosticsRoot::open(&state, "test-connection").map_err(|error| format!("{error:?}"))?;
    let key = OperationKey {
        journal_identity: root.journal_identity.clone(),
        connection_id: "other-connection".to_owned(),
        intent_id: 17,
    };
    if !matches!(
        RunnerLogStageRequest::new(
            &root,
            key,
            4,
            &"a".repeat(64),
            &valid_archive()?,
            Instant::now(),
        ),
        Err(HostError::Ownership)
    ) {
        return Err("foreign operation key was accepted".to_owned());
    }
    drop(root);
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    let _ = LOG_PATH;
    Ok(())
}
