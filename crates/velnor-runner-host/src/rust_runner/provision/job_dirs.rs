// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::super::machine::failure;
use super::super::{GUEST_GEN, JobPaths, ReservationId, RustRunnerLauncher, orb};
use super::parse;
use crate::error::{HostError, RustRunnerFailure};

const WEDGED_REMOVAL: i32 = 70;
const WEDGED_REMAINS: i32 = 71;
const WEDGED_MOUNT: i32 = 72;
const WEDGED_TABLE: i32 = 73;
const MOUNTED: &str = r#"table=$(findmnt -rn -o TARGET) || exit 73; test -n "$table" || exit 73; mounted() { printf '%s\n' "$table" | awk -v dir="$1" '$0 == dir || index($0, dir "/") == 1 { found = 1 } END { exit !found }'; verdict=$?; if test "$verdict" -gt 1; then exit 73; fi; return "$verdict"; }"#;

pub(super) fn remover() -> String {
    format!(
        r#"{MOUNTED}; for dir in "$@"; do if mounted "$dir"; then exit {WEDGED_MOUNT}; fi; done; rm -rf -- "$@" || exit {WEDGED_REMOVAL}; for entry in "$@"; do if test -e "$entry" || test -L "$entry"; then exit {WEDGED_REMAINS}; fi; done"#
    )
}

pub(super) fn sweeper() -> String {
    format!(
        r#"{MOUNTED}; for dir in "$@"; do for entry in "$dir"/* "$dir"/.[!.]* "$dir"/..?*; do test -e "$entry" || test -L "$entry" || continue; if mounted "$entry"; then continue; fi; find "$entry" -inum 256 -exec btrfs property set -ts {{}} ro false \; ; rm -rf -- "$entry"; done; done; find "$@" -mindepth 1 -maxdepth 1 -printf '%f\n'"#
    )
}

impl RustRunnerLauncher {
    pub(crate) fn remove_job_dirs(&self, paths: &JobPaths) -> Result<(), HostError> {
        self.job_dirs(
            &remover(),
            &[paths.guest_work_dir(), paths.guest_stage_dir()],
        )
    }

    pub(crate) fn sweep_job_dirs(&self) -> Result<Vec<ReservationId>, HostError> {
        let work = format!("{GUEST_GEN}/work");
        let stage = format!("{GUEST_GEN}/stage");
        let swept = self.read(&["sh", "-c", &sweeper(), "sh", &work, &stage])?;
        if !swept.success() {
            return Err(failure(
                removal_verdict(swept.code)
                    .err()
                    .unwrap_or(RustRunnerFailure::JobFilesLeft),
            ));
        }
        survivors(&swept.text)
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
        Some(WEDGED_REMOVAL | WEDGED_REMAINS | WEDGED_MOUNT | WEDGED_TABLE) => {
            Err(RustRunnerFailure::JobDirsWedged)
        }
        _ => Err(RustRunnerFailure::JobFilesLeft),
    }
}

fn survivors(listing: &str) -> Result<Vec<ReservationId>, HostError> {
    let numbers: Result<BTreeSet<u64>, HostError> = parse::lines(listing)
        .into_iter()
        .map(|name| {
            name.parse()
                .ok()
                .filter(|number| *number > 0)
                .ok_or_else(|| failure(RustRunnerFailure::JobDirsWedged))
        })
        .collect();
    Ok(numbers?
        .into_iter()
        .filter_map(ReservationId::new)
        .collect())
}
