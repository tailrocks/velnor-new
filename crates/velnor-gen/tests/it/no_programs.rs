// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::process::Command;

#[test]
fn no_program_can_be_started_by_a_test() {
    // The no-programs wrapper in .config/nextest.toml sets PATH=/nonexistent.
    // Plain `cargo test` has no wrapper, so the check cannot hold there and is
    // skipped. Under nextest a missing wrapper is a failure.
    if std::env::var_os("PATH").as_deref() != Some(std::ffi::OsStr::new("/nonexistent")) {
        assert!(
            std::env::var_os("NEXTEST").is_none(),
            "nextest ran this test without the no-programs wrapper"
        );
        eprintln!("skipped: needs `cargo nextest run` (the no-programs wrapper sets PATH)");
        return;
    }
    for program in ["git", "cargo", "sh"] {
        let error = Command::new(program)
            .arg("--version")
            .output()
            .err()
            .unwrap_or_else(|| unreachable!("{program} started"));
        assert_eq!(error.kind(), ErrorKind::NotFound, "{program}");
    }
}
