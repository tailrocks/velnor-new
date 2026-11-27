// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::machine::failure;
use crate::rust_runner::{GIB, GUEST_CAP_RECORD, RustRunnerLauncher, orb, shape};

const CAP_PROBE_FILE: &str = "/var/lib/velnor/.cap-probe";
const CAP_PROBE: &str = r#"fallocate -l "$1" -- "$2"; status=$?; rm -f -- "$2"; exit $status"#;
const RECORD_WRITER: &str = r#"printf '%s\n' "$1" > "$2.new" && mv -f "$2.new" "$2""#;
const QUOTA_REFUSALS: [&str; 2] = ["No space left on device", "Disk quota exceeded"];

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
        let over_cap = self.disk_cap_bytes().saturating_add(GIB).to_string();
        let attempt = self.read(&["sh", "-c", CAP_PROBE, "sh", &over_cap, CAP_PROBE_FILE])?;
        let enforced = !attempt.success()
            && QUOTA_REFUSALS
                .iter()
                .any(|refusal| attempt.stderr.contains(refusal));
        if !enforced {
            return Err(failure(RustRunnerFailure::CapProbe));
        }
        self.apply(&["sh", "-c", RECORD_WRITER, "sh", &record, GUEST_CAP_RECORD])
    }
}
