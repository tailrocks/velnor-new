// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use crate::io::Repo;

#[test]
fn a_run_error_naming_a_runner_temp_with_a_line_break_stays_on_one_line() {
    let repo = Repo::new("run-escape-temp");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "true"])
        .current_dir(repo.root())
        .env(
            "RUNNER_TEMP",
            repo.root().join("missing\n::stop-commands::token"),
        )
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stderr}"
    );
    assert!(stderr.contains("%0A::stop-commands::token"), "{stderr}");
}
