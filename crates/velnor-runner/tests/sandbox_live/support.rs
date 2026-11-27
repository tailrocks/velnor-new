// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub(crate) const UID: u32 = 5001;
pub(crate) const GID: u32 = 5002;
pub(crate) const HOME: &str = "/home/live";
pub(crate) const GENERATIONS: &str = "/srv/velnor/gen";
pub(crate) const RUNNER: &str = env!("CARGO_BIN_EXE_velnor-runner");

pub(crate) struct Sandbox {
    pub(crate) reservation: u64,
    arguments: Vec<String>,
}

pub(crate) fn require_live_environment() {
    assert!(
        rustix::process::geteuid().is_root()
            && Path::new("/var/lib/velnor/tools").is_dir()
            && Path::new(GENERATIONS).is_dir(),
        "the live sandbox lane needs root, /var/lib/velnor/tools and /srv/velnor/gen; provision it with scripts/velnor-sandbox-live.ts"
    );
}

impl Sandbox {
    pub(crate) fn new(name: &str) -> Self {
        require_live_environment();
        let reservation = 1_000_000
            + name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
            }) % 1_000_000;
        Self {
            reservation,
            arguments: Vec::new(),
        }
    }

    pub(crate) fn with(mut self, flag: &str, value: &str) -> Self {
        self.arguments.push(flag.to_owned());
        self.arguments.push(value.to_owned());
        self
    }

    pub(crate) fn work_dir(&self) -> PathBuf {
        Path::new(GENERATIONS)
            .join("work")
            .join(self.reservation.to_string())
    }

    pub(crate) fn command(&self, command: &[&str]) -> Command {
        let mut process = Command::new(RUNNER);
        process
            .args(["sandbox", "--profile", "job", "--reservation"])
            .arg(self.reservation.to_string())
            .args(["--uid", &UID.to_string(), "--gid", &GID.to_string()])
            .args(["--home", HOME])
            .arg("--work-dir")
            .arg(self.work_dir())
            .args(&self.arguments)
            .arg("--")
            .args(command)
            .stdin(Stdio::null());
        process
    }

    pub(crate) fn run(&self, command: &[&str]) -> Output {
        unstarted(self.command(command).output())
    }

    pub(crate) fn shell(&self, script: &str) -> Output {
        self.run(&["/bin/sh", "-c", script])
    }
}

pub(crate) fn unstarted(result: std::io::Result<Output>) -> Output {
    result.unwrap_or_else(|error| Output {
        status: ExitStatusExt::from_raw(255 << 8),
        stdout: Vec::new(),
        stderr: format!("the process did not start: {error}").into_bytes(),
    })
}

pub(crate) fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(crate) fn succeeded(output: &Output) -> String {
    assert!(output.status.success(), "{}", text(output));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub(crate) fn failed(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "the command succeeded but had to fail: {}",
        text(output)
    );
    text(output)
}
