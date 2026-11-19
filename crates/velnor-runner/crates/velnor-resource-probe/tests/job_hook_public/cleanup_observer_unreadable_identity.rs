// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, process::Command};

use super::{TestResult, identity, process, scan_runner_identity};

#[test]
fn an_unreadable_pair_identity_does_not_exempt_observer_runtime_mounts() -> TestResult<()> {
    const MARKER: &str = "VELNOR_TEST_UNREADABLE_PAIR_IDENTITY";
    if std::env::var_os(MARKER).is_none() {
        let mut command = Command::new("unshare");
        if rustix::process::geteuid().is_root() {
            command.arg("--mount");
        } else {
            command.args(["--user", "--map-root-user", "--mount"]);
        }
        let output = command
            .args([
                "--propagation",
                "private",
                "sh",
                "-c",
                "mount --bind /proc/$$/mem /proc/$$/stat && VELNOR_TEST_UNREADABLE_PAIR_IDENTITY=$$ \"$1\" --exact cleanup_observer_scan::cleanup_observer_unreadable_identity::an_unreadable_pair_identity_does_not_exempt_observer_runtime_mounts --nocapture; status=$?; exit \"$status\"",
                "sh",
            ])
            .arg(std::env::current_exe()?)
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "{output:?}"
        );
        return Ok(());
    }
    let pid = std::env::var(MARKER)?.parse()?;
    let error =
        fs::read(format!("/proc/{pid}/stat")).expect_err("injected identity read must fail");
    assert_eq!(error.raw_os_error(), Some(5), "{error:?}");
    let own = identity("/proc/self/ns/mnt")?;
    let record = scan_runner_identity(process(pid, own))?;
    assert!(
        record
            .issues
            .iter()
            .any(|issue| issue == "observer_identity_unreadable"),
        "{record:?}"
    );
    assert!(record.owned_reference_count >= 1, "{record:?}");
    assert!(
        !record.complete && !record.owned_references_absent,
        "{record:?}"
    );
    Ok(())
}
