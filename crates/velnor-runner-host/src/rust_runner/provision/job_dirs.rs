// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{GUEST_GEN, JobPaths, RustRunnerLauncher};
use crate::error::HostError;

impl RustRunnerLauncher {
    pub(crate) fn remove_job_dirs(&self, paths: &JobPaths) -> Result<(), HostError> {
        self.apply(&[
            "rm",
            "-rf",
            "--one-file-system",
            "--",
            paths.guest_work_dir(),
            paths.guest_stage_dir(),
        ])
    }

    pub(crate) fn job_dirs_remain(&self, paths: &JobPaths) -> Result<bool, HostError> {
        for dir in [paths.guest_work_dir(), paths.guest_stage_dir()] {
            if self.read(&["find", dir, "-maxdepth", "0"])?.success() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn empty_job_dirs(&self) -> Result<(), HostError> {
        for kind in ["work", "stage"] {
            let dir = format!("{GUEST_GEN}/{kind}");
            self.apply(&[
                "find",
                &dir,
                "-mindepth",
                "1",
                "-maxdepth",
                "1",
                "-exec",
                "rm",
                "-rf",
                "--one-file-system",
                "--",
                "{}",
                "+",
            ])?;
        }
        Ok(())
    }
}
