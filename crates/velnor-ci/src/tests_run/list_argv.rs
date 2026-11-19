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

fn plain_run_with(extra: &[&str]) -> Vec<String> {
    let mut run = argv(&["cargo", "nextest", "run", "--workspace"]);
    run.extend(argv(extra));
    run.push("--no-fail-fast".to_owned());
    run
}

#[test]
fn cargo_message_format_is_dropped_in_every_spelling() {
    for extra in [
        &["--cargo-message-format", "json-render-diagnostics"][..],
        &["--cargo-message-format=json-render-diagnostics"][..],
        &["--message-format", "libtest-json"][..],
        &["--message-format=libtest-json"][..],
    ] {
        assert_eq!(
            list_argv(&plain_run_with(extra)),
            Some(argv(&[
                "cargo",
                "nextest",
                "list",
                "--workspace",
                "--message-format",
                "json",
            ])),
            "{extra:?}"
        );
    }
}

#[test]
fn cargo_message_format_before_the_filter_keeps_the_filter() {
    let run = argv(&[
        "cargo",
        "nextest",
        "run",
        "--cargo-message-format",
        "json-render-diagnostics",
        "-E",
        "all()",
    ]);

    assert_eq!(
        list_argv(&run),
        Some(argv(&[
            "cargo",
            "nextest",
            "list",
            "-E",
            "all()",
            "--message-format",
            "json",
        ]))
    );
}
