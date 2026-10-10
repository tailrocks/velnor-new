use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;
use std::time::Duration;

use crate::error::HostError;

use super::run_helper;

struct TestScript {
    directory: PathBuf,
    path: PathBuf,
}

impl TestScript {
    fn new(contents: &str) -> Result<Self, HostError> {
        let directory =
            std::env::temp_dir().join(format!("velnor-helper-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|_| HostError::Path)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        let path = directory.join("helper.sh");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| HostError::Path)?;
        file.write_all(contents.as_bytes())
            .map_err(|_| HostError::Path)?;
        file.sync_all().map_err(|_| HostError::Path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self { directory, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestScript {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!("test helper directory cleanup failed: {error}");
        }
    }
}

#[tokio::test]
async fn accepts_only_the_exact_success_response() -> Result<(), HostError> {
    let helper =
        TestScript::new("#!/bin/sh\nprintf '%s' '{\"schema\":1,\"result\":\"verified\"}'\n")?;
    run_helper(helper.path(), b"{}".to_vec()).await
}

#[tokio::test]
async fn rejects_success_with_extra_response_fields() -> Result<(), HostError> {
    let helper = TestScript::new(
        "#!/bin/sh\nprintf '%s' '{\"schema\":1,\"result\":\"verified\",\"ok\":true}'\n",
    )?;
    assert_eq!(
        run_helper(helper.path(), b"{}".to_vec()).await,
        Err(HostError::Identity)
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_the_caller_kills_and_reaps_the_helper() -> Result<(), HostError> {
    let helper =
        TestScript::new("#!/bin/sh\nprintf '%s' \"$$\" > '$PID_PATH'\nexec /bin/sleep 30\n")?;
    let pid_path = helper.directory.join("pid");
    let script = fs::read_to_string(helper.path()).map_err(|_| HostError::Path)?;
    let script = script.replace("$PID_PATH", &shell_quote(&pid_path));
    fs::write(helper.path(), script).map_err(|_| HostError::Path)?;
    fs::set_permissions(helper.path(), fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Path)?;
    let path = helper.path().to_owned();
    let task = tokio::spawn(async move { run_helper(&path, b"{}".to_vec()).await });
    let pid = wait_for_pid(&pid_path, &task).await?;
    task.abort();
    match task.await {
        Err(error) if error.is_cancelled() => {}
        _ => return Err(HostError::Identity),
    }
    wait_until_reaped(pid).await
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

async fn wait_for_pid(
    pid_path: &Path,
    task: &tokio::task::JoinHandle<Result<(), HostError>>,
) -> Result<u32, HostError> {
    for _ in 0..200 {
        if let Ok(value) = fs::read_to_string(pid_path) {
            return value.parse().map_err(|_| HostError::Identity);
        }
        if task.is_finished() {
            return Err(HostError::Identity);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(HostError::DockerTimeout)
}

async fn wait_until_reaped(pid: u32) -> Result<(), HostError> {
    for _ in 0..200 {
        let status = OsCommand::new("/bin/kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map_err(|_| HostError::Identity)?;
        if !status.success() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(HostError::DockerTimeout)
}
