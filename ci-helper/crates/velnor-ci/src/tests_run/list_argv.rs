// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::list_argv;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn audited_run_lists_with_the_nextest_command_the_wrapper_runs() {
    let run = argv(&[
        "velnor-ci",
        "inputs",
        "build",
        "--",
        "cargo-nextest",
        "nextest",
        "run",
        "--workspace",
        "--locked",
        "--no-fail-fast",
        "--ignore-default-filter",
        "--status-level",
        "pass",
        "-E",
        "all()",
        "--no-tests=pass",
    ]);

    assert_eq!(
        list_argv(&run),
        Some(argv(&[
            "cargo-nextest",
            "nextest",
            "list",
            "--workspace",
            "--locked",
            "--ignore-default-filter",
            "-E",
            "all()",
            "--message-format",
            "json",
        ]))
    );
}

#[test]
fn plain_run_lists_with_its_own_program() {
    let run = argv(&["cargo", "nextest", "run", "--workspace", "--no-fail-fast"]);

    assert_eq!(
        list_argv(&run),
        Some(argv(&[
            "cargo",
            "nextest",
            "list",
            "--workspace",
            "--message-format",
            "json",
        ]))
    );
}
