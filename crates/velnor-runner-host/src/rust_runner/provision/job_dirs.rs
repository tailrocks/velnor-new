// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::machine::failure;
use super::super::{GUEST_GEN, JobPaths, RustRunnerLauncher, orb};
use crate::error::{HostError, RustRunnerFailure};

const WEDGED_REMOVAL: i32 = 70;
const WEDGED_REMAINS: i32 = 71;

fn remover() -> String {
    format!(
        r#"rm -rf -- "$@" || exit {WEDGED_REMOVAL}; for entry in "$@"; do if test -e "$entry" || test -L "$entry"; then exit {WEDGED_REMAINS}; fi; done"#
    )
}

fn emptier() -> String {
    format!(
        r#"for dir in "$@"; do find "$dir" -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} + || exit {WEDGED_REMOVAL}; done"#
    )
}

impl RustRunnerLauncher {
    pub(crate) fn remove_job_dirs(&self, paths: &JobPaths) -> Result<(), HostError> {
        self.job_dirs(
            &remover(),
            &[paths.guest_work_dir(), paths.guest_stage_dir()],
        )
    }

    pub(crate) fn empty_job_dirs(&self) -> Result<(), HostError> {
        let work = format!("{GUEST_GEN}/work");
        let stage = format!("{GUEST_GEN}/stage");
        self.job_dirs(&emptier(), &[&work, &stage])
    }

    fn job_dirs(&self, script: &str, dirs: &[&str]) -> Result<(), HostError> {
        let mut command = self.guest();
        command.args(["sh", "-c", script, "sh"]).args(dirs);
        let finished = orb::run(command, self.deadline, false).map_err(failure)?;
        removal_verdict(finished.status.code()).map_err(failure)
    }
}

pub(super) const fn removal_verdict(code: Option<i32>) -> Result<(), RustRunnerFailure> {
    match code {
        Some(0) => Ok(()),
        Some(WEDGED_REMOVAL | WEDGED_REMAINS) => Err(RustRunnerFailure::JobDirsWedged),
        _ => Err(RustRunnerFailure::JobFilesLeft),
    }
}
