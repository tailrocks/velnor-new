// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::process::Command;

#[test]
fn no_program_can_be_started_by_a_test() {
    for program in ["git", "cargo", "sh"] {
        let error = Command::new(program)
            .arg("--version")
            .output()
            .err()
            .unwrap_or_else(|| unreachable!("{program} started"));
        assert_eq!(error.kind(), ErrorKind::NotFound, "{program}");
    }
}
