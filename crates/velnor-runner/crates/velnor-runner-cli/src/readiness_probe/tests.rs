use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use super::{ChildWait, INTERNAL_FLAG, Readiness, bounded_wait, read_status, readiness_command};

#[tokio::test]
async fn readiness_child_accepts_ready_status_from_the_internal_observer() -> Result<(), String> {
    let mut child = tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("printf 'ready\\n'")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("The readiness child test fixture failed.".to_owned());
    }
    let readiness = read_status(
        &mut child,
        tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await;
    if readiness != Readiness::Ready {
        return Err("The readiness parser did not accept the ready status.".to_owned());
    }
    Ok(())
}

#[test]
fn readiness_child_uses_only_the_private_operation_and_state_path() -> Result<(), String> {
    let state = PathBuf::from("/private/controller/state");
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = readiness_command(state.clone()).map_err(|error| error.to_string())?;
    let standard = command.as_std();
    if standard.get_program() != executable.as_os_str() {
        return Err("readiness child executable changed".to_owned());
    }
    if standard.get_args().collect::<Vec<_>>().as_slice()
        != [OsStr::new(INTERNAL_FLAG), state.as_os_str()]
    {
        return Err("readiness child received unexpected arguments".to_owned());
    }
    if standard.get_envs().next().is_some() {
        return Err("readiness child received environment overrides".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn expired_readiness_child_is_killed_and_reaped() -> Result<(), String> {
    let mut child = tokio::process::Command::new("/bin/sleep")
        .arg("30")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let pid = child.id().ok_or("missing child pid")?;
    let now = tokio::time::Instant::now();
    let result = bounded_wait(
        &mut child,
        now + Duration::from_millis(20),
        now + Duration::from_secs(2),
    )
    .await;
    if !matches!(result, ChildWait::TimedOut) {
        return Err("child did not time out".to_owned());
    }
    let pid = pid.to_string();
    let status = Command::new("/bin/kill")
        .args(["-0", pid.as_str()])
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        return Err("timed-out child still exists".to_owned());
    }
    Ok(())
}
