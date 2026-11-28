// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::machine::failure;
use crate::rust_runner::{GIB, GUEST_CAP_RECORD, GUEST_GEN, RustRunnerLauncher, orb, shape};

const CAP_PROBE_FILE: &str = "/var/lib/velnor/.cap-probe";
const CAP_PROBE: &str = r#"rm -f -- "$2"; (set -C; : > "$2") || exit 1; fallocate -l "$1" -- "$2"; status=$?; rm -f -- "$2"; exit $status"#;
const RECORD_WRITER: &str =
    r#"rm -f -- "$2.new" && (set -C; printf '%s\n' "$1" > "$2.new") && mv -fT -- "$2.new" "$2""#;
const NO_SPACE: &str = "No space left on device";
const QUOTA_EXCEEDED: &str = "Disk quota exceeded";

impl RustRunnerLauncher {
    fn cap_record(&self) -> Result<String, HostError> {
        let mut command = self.orb();
        command.arg("version");
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        let version = shape::orb_version(&String::from_utf8_lossy(&output.stdout))
            .filter(|_| output.status.success())
            .ok_or_else(|| failure(RustRunnerFailure::MachineProvision))?;
        Ok(format!("orbstack={version} cap={}", self.disk_cap_bytes()))
    }

    pub(super) fn ensure_cap(&self) -> Result<(), HostError> {
        let record = self.cap_record()?;
        let recorded = self.read(&["cat", GUEST_CAP_RECORD])?;
        if recorded.success() && recorded.text.trim() == record {
            return Ok(());
        }
        let over_cap = self.disk_cap_bytes().saturating_add(GIB);
        let attempt = self.read(&[
            "sh",
            "-c",
            CAP_PROBE,
            "sh",
            &over_cap.to_string(),
            CAP_PROBE_FILE,
        ])?;
        let quota = attempt.stderr.contains(QUOTA_EXCEEDED);
        if attempt.success() || !(quota || attempt.stderr.contains(NO_SPACE)) {
            return Err(failure(RustRunnerFailure::CapProbe));
        }
        if !quota && !self.has_room_for(over_cap)? {
            return Err(failure(RustRunnerFailure::MachineShape));
        }
        self.apply(&["sh", "-c", RECORD_WRITER, "sh", &record, GUEST_CAP_RECORD])
    }

    fn has_room_for(&self, probe: u64) -> Result<bool, HostError> {
        let usage = self.read(&["btrfs", "filesystem", "usage", "-b", GUEST_GEN])?;
        Ok(usage.success()
            && parse::free_bytes(&usage.text)
                .is_some_and(|free| free >= probe.saturating_add(probe)))
    }
}
