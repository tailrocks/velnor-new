// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use uuid::Uuid;
use velnor_runner::exec::{RunOptions, TOOL_CACHE_VARIABLES};
use velnor_runner::job_message::AgentJobRequestMessage;
use velnor_runner::protocol::RunnerOs;

pub(crate) fn host_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub(crate) fn new() -> Result<Self> {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("exec-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).context("create the scratch directory")?;
        Ok(Self { root })
    }

    pub(crate) fn work_dir(&self) -> PathBuf {
        self.root.join("_work")
    }

    pub(crate) fn write(&self, name: &str, content: &[u8]) -> Result<PathBuf> {
        let path = self.root.join(name);
        std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
        Ok(path)
    }

    pub(crate) fn workspace(&self) -> PathBuf {
        self.root.join("workspace")
    }

    pub(crate) fn options(&self) -> RunOptions {
        RunOptions {
            workspace: self.workspace(),
            temp: self.root.join("temp"),
            tool_cache: self.root.join("_tool"),
            runner_name: "exec-test".to_owned(),
            os: RunnerOs::Linux,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
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

pub(crate) fn scrub_tool_cache(command: &mut Command) {
    for name in TOOL_CACHE_VARIABLES {
        command.env_remove(name);
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-runner"));
    scrub_tool_cache(&mut command);
    let output = command
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
