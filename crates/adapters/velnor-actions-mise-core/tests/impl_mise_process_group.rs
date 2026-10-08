//! Process-group cleanup cases for task commands that spawn descendants.
#![cfg(unix)]

use std::ffi::OsString;
use std::time::Duration;
use velnor_actions_mise_core::MiseError;
use velnor_actions_mise_core::command::{
    IsolatedCommand, is_cancel_or_timeout, is_reserved_env_key,
};

#[test]
fn successful_parent_cannot_leave_a_pipe_holding_descendant_running() -> Result<(), String> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-pipe-descendant-{}-{time}",
        std::process::id()
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let marker = root.join("descendant-survived");
    let script = format!(
        "(sleep 0.35; printf leaked > '{}') & exit 0",
        marker.display()
    );
    let task = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
    .map_err(|error| error.to_string())?;
    let started = std::time::Instant::now();
    let error = task
        .run_bounded(1024, Duration::from_millis(100))
        .expect_err("pipe inheritance must remain under the deadline");
    assert!(
        is_cancel_or_timeout(&error),
        "timeout must classify: {error}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(400));
    let survived = marker.exists();
    std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    assert!(
        !survived,
        "timeout cleanup kills descendants in its process group"
    );
    Ok(())
}

#[test]
fn hook_escape_privileged_declared_keys_never_run() {
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(is_reserved_env_key(key), "{key} must be reserved");
        let declared = vec![(OsString::from(key), OsString::from("hostile"))];
        assert!(
            matches!(
                IsolatedCommand::repo_task("sh", Vec::new(), &declared),
                Err(MiseError::InvalidStepInput { .. })
            ),
            "project task declaring {key} must fail before spawn"
        );
    }
}

#[test]
fn streaming_timeout_kills_pipe_holding_descendant_after_parent_exit() -> Result<(), String> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-stream-pipe-descendant-{}-{time}",
        std::process::id()
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let marker = root.join("descendant-survived");
    let script = format!(
        "(sleep 0.35; printf leaked > '{}') & exit 0",
        marker.display()
    );
    let task = IsolatedCommand::direct(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
    );
    let started = std::time::Instant::now();
    let error = task
        .run_stdout_to(1024, Duration::from_millis(100), |_| Ok(()))
        .expect_err("an inherited pipe must not extend the deadline");
    assert!(
        is_cancel_or_timeout(&error),
        "stream timeout must classify: {error}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(400));
    let survived = marker.exists();
    std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    assert!(
        !survived,
        "timeout cleanup kills the still-owned process group after parent exit"
    );
    Ok(())
}
