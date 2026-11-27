// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use uuid::Uuid;
use velnor_runner::exec::RunOptions;
use velnor_runner::job_message::AgentJobRequestMessage;

pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub(crate) fn new() -> Result<Self> {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("exec-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).context("create the scratch directory")?;
        Ok(Self { root })
    }

    pub(crate) fn workspace(&self) -> PathBuf {
        self.root.join("workspace")
    }

    pub(crate) fn options(&self) -> RunOptions {
        RunOptions {
            workspace: self.workspace(),
            temp: self.root.join("temp"),
            runner_name: "exec-test".to_owned(),
        }
    }
}

pub(crate) fn unlock_tree(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        for item in std::fs::read_dir(path).into_iter().flatten().flatten() {
            unlock_tree(&item.path());
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        unlock_tree(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(crate) struct Ran {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) code: Option<i32>,
    pub(crate) elapsed: Duration,
}

impl Ran {
    pub(crate) fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }

    pub(crate) fn has_line(&self, expected: &str) -> bool {
        self.stdout.lines().any(|line| line == expected)
    }
}

pub(crate) fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/exec")
        .join(format!("{name}.json"))
}

pub(crate) fn fixture_message(name: &str) -> Result<AgentJobRequestMessage> {
    let body = std::fs::read_to_string(fixture_path(name)).context("read the fixture")?;
    AgentJobRequestMessage::parse_json(&body)
}

pub(crate) fn run_fixture(name: &str, scratch: &Scratch) -> Result<Ran> {
    run_path(&fixture_path(name), scratch)
}

pub(crate) fn run_path(job: &Path, scratch: &Scratch) -> Result<Ran> {
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .arg("exec")
        .arg("--job")
        .arg(job)
        .arg("--workspace")
        .arg(scratch.workspace())
        .arg("--temp")
        .arg(scratch.root.join("temp"))
        .output()
        .context("run velnor-runner exec")?;
    Ok(Ran {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
        elapsed: started.elapsed(),
    })
}
