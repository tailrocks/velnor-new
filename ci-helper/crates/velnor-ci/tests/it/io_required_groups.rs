// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

const SPEC: &str = "--plan rust-plan --job rust-plan=always --job rust-test=lane:root --job rust-test-db=lane:root+group:root:db --tests rust-test=root --tests rust-test-db=root";

fn counts(run: &str) -> String {
    format!(
        r#"{{"result":"success","outputs":{{"tests_run":"{run}","tests_listed":"{run}","tests_by_package":"pkg={run}/{run}"}}}}"#
    )
}

fn gate(group: &str, db_result: &str) -> (Option<i32>, String, String) {
    let db = if db_result == "skipped" {
        r#"{"result":"skipped","outputs":{}}"#.to_owned()
    } else {
        counts("3")
    };
    let needs = format!(
        r#"{{"rust-plan":{{"result":"success","outputs":{{"rust_root_mode":"some","rust_root_tests":"run","rust_root_group_db":"{group}"}}}},"rust-test":{},"rust-test-db":{db}}}"#,
        counts("7")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["gate", "--spec-env", "GATE_SPEC"])
        .env("GATE_SPEC", SPEC)
        .env("NEEDS_JSON", needs)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_group_the_plan_skips_must_be_skipped_and_a_group_it_runs_must_succeed() {
    let (code, table, _) = gate("skip", "skipped");
    assert_eq!(code, Some(0), "{table}");
    assert!(
        table.contains("| rust-test-db | not scheduled | skipped |"),
        "{table}"
    );
    let (code, table, _) = gate("run", "success");
    assert_eq!(code, Some(0), "{table}");
    assert!(
        table.contains("| rust-test-db | scheduled | success |"),
        "{table}"
    );
    let (code, table, stderr) = gate("run", "skipped");
    assert_eq!(code, Some(1), "{table}");
    assert!(
        stderr.contains("rust-test-db was scheduled by the plan and its result is skipped"),
        "{stderr}"
    );
}

#[test]
fn a_group_output_that_is_neither_run_nor_skip_fails_closed() {
    let (code, _, stderr) = gate("maybe", "skipped");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("rust_root_group_db is `maybe`, not run or skip"),
        "{stderr}"
    );
}
