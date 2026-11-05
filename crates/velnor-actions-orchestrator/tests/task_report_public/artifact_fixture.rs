// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) fn dispatch(test: &str) -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../velnor-actions-cli/tests/api2026_pull_context_fixture/driver.py"
        ))
        .args([
            "task_report",
            test,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        ])
        .status()?;
    if !status.success() {
        return Err(format!("isolated artifact fixture failed: {status}").into());
    }
    Ok(())
}

pub(super) struct Server {
    child: Child,
    _lock: fs::File,
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

pub(super) fn start(
    state: &Path,
    payload: &Path,
    artifact: &str,
) -> Result<Server, Box<dyn Error>> {
    let tls = env::var_os("VELNOR_GH_TEST_TLS_ROOT").ok_or("isolated TLS fixture missing")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open("/tmp/velnor-gh-fixture.lock")?;
    lock.lock()?;
    fs::write(state.join("mode"), "valid")?;
    let script = state.join("artifact-server.py");
    fs::write(&script, include_str!("artifact_server.py"))?;
    let mut child = Command::new("/usr/bin/python3")
        .args([
            script.as_os_str(),
            state.as_os_str(),
            payload.as_os_str(),
            &tls,
            artifact.as_ref(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !state.join("ready").exists() {
        if let Some(status) = child.try_wait()? {
            return Err(format!("artifact fixture exited: {status}").into());
        }
        if Instant::now() >= deadline {
            drop(child.kill());
            drop(child.wait());
            return Err("artifact fixture startup timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(Server { child, _lock: lock })
}

pub(super) fn assert_negative_cases(scenario: &super::fixture::Scenario) -> super::TestResult<()> {
    for mode in ["missing", "missing-zip", "wrong-id", "corrupt"] {
        fs::write(scenario.root().join("mode"), mode)?;
        assert_eq!(
            super::report_files::retrieve(
                scenario,
                super::SHARED_ARTIFACT_TEST,
                mode,
                "tailrocks/velnor-new"
            )?,
            0
        );
    }
    fs::write(scenario.root().join("mode"), "valid")?;
    assert_eq!(
        super::report_files::retrieve(
            scenario,
            super::SHARED_ARTIFACT_TEST,
            "wrong-repo",
            "other/repository"
        )?,
        0
    );
    Ok(())
}
