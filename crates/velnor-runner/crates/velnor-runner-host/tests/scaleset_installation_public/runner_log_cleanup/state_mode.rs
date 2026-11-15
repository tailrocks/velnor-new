// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{CompletionWorker, DaemonLock, HostError, connect_unix};

use super::super::super::TestResult;
use super::super::super::runtime_and_policy::sample_connection;
use super::{WorkflowEvent, seed, spawn_docker_stub};

#[test]
fn connect_creates_a_missing_state_folder_private() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?.join("state");
    let lock = DaemonLock::try_acquire(&state.join("config.lock"))?;
    let mode = std::fs::metadata(&state)?.permissions().mode() & 0o777;
    drop(lock);
    assert_eq!(mode, 0o700);
    Ok(())
}

#[test]
fn completion_start_on_a_group_readable_state_folder_returns_an_error() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let state_dir = temp.path().canonicalize()?;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o755))?;
    let socket_path = state_dir.join("docker.sock");
    let _stub = spawn_docker_stub(&socket_path, 0, Vec::new())?;
    let connection = sample_connection(vec![WorkflowEvent::Push], Vec::new())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let outcome = runtime.block_on(async {
        let journal =
            seed::seed_completed_launch(&state_dir.join("journal.db"), &connection).await?;
        let docker = connect_unix(socket_path.to_str().ok_or("socket utf8")?)?;
        let started = CompletionWorker::start_for_connection(
            &journal,
            &connection,
            docker,
            "https://api.github.com",
            "ghs_test_admin_token".to_owned(),
            "ghp_test_release_read_token",
            &state_dir,
        )
        .await;
        Ok::<_, Box<dyn std::error::Error>>(started.map(drop))
    })?;
    assert!(matches!(outcome, Err(HostError::Path)));
    Ok(())
}
