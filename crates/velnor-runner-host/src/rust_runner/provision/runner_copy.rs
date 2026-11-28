// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;

use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::machine::failure;
use crate::rust_runner::{GUEST_RUNNER, RustRunnerLauncher, orb, runner_binary};

const RUNNER_STAGE: &str = "/var/lib/velnor/bin/.stage";
const STAGE_WRITER: &str = r#"set -e; umask 077; rm -f -- "$1"; cat > "$1"; chmod 0700 -- "$1""#;

impl RustRunnerLauncher {
    pub fn installed_runner_digest(&self) -> Result<String, HostError> {
        self.installed_digest()?
            .ok_or_else(|| failure(RustRunnerFailure::BinaryCopy))
    }

    fn installed_digest(&self) -> Result<Option<String>, HostError> {
        let owned = self
            .stat(&[GUEST_RUNNER])?
            .get(GUEST_RUNNER)
            .is_some_and(|entry| entry.is(super::REGULAR_FILE, 0o755));
        if owned {
            self.sha256_of(GUEST_RUNNER)
        } else {
            Ok(None)
        }
    }

    fn sha256_of(&self, path: &str) -> Result<Option<String>, HostError> {
        let sum = self.read(&["sha256sum", path])?;
        Ok(sum.success().then(|| parse::digest(&sum.text)).flatten())
    }

    pub(super) fn refresh_runner(&self, digest: &str) -> Result<(), HostError> {
        if self.installed_digest()?.as_deref() == Some(digest) {
            return Ok(());
        }
        let staged = self.stage_runner(digest);
        if staged.is_err() {
            let _ = self.apply(&["rm", "-f", "--", RUNNER_STAGE]);
        }
        staged
    }

    fn stage_runner(&self, digest: &str) -> Result<(), HostError> {
        let source = runner_binary::open(&self.runner_binary)?;
        let mut command = self.guest();
        command.args(["sh", "-c", STAGE_WRITER, "sh", RUNNER_STAGE]);
        let streamed = orb::run_with_stdin(command, self.deadline, false, Stdio::from(source))
            .map_err(failure)?;
        if !streamed.status.success() {
            return Err(failure(RustRunnerFailure::MachineProvision));
        }
        if self.sha256_of(RUNNER_STAGE)?.as_deref() != Some(digest) {
            return Err(failure(RustRunnerFailure::BinaryCopy));
        }
        self.apply(&["chmod", "0755", "--", RUNNER_STAGE])?;
        self.apply(&["mv", "-fT", "--", RUNNER_STAGE, GUEST_RUNNER])?;
        if self.installed_digest()?.as_deref() == Some(digest) {
            Ok(())
        } else {
            let _ = self.apply(&["rm", "-f", "--", GUEST_RUNNER]);
            Err(failure(RustRunnerFailure::BinaryCopy))
        }
    }
}
