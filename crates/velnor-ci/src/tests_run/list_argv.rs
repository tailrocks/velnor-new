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

fn listed_without_message_format(prefix: &[&str], program: &[&str]) -> Option<Vec<String>> {
    let mut run = argv(prefix);
    run.extend(argv(program));
    run.extend(argv(&[
        "--workspace",
        "--locked",
        "--cargo-message-format=json-render-diagnostics",
        "--profile",
        "ci",
        "--no-fail-fast",
        "-E",
        "all()",
        "--no-tests=pass",
    ]));
    list_argv(&run)
}

#[test]
fn every_wrapped_spelling_of_the_generated_test_step_lists_without_the_cargo_message_format() {
    for prefix in [
        &[][..],
        &["velnor-ci", "inputs", "build", "--"][..],
        &["/tmp/trusted/velnor-ci", "inputs", "build", "--"][..],
        &[
            "velnor-ci",
            "inputs",
            "build",
            "--directory",
            "../shared",
            "--",
        ][..],
        &[
            "velnor-ci",
            "inputs",
            "build",
            "--directory=../shared",
            "--",
        ][..],
    ] {
        for program in [
            &["cargo", "nextest", "run"][..],
            &["cargo-nextest", "nextest", "run"][..],
            &["/home/runner/.cargo/bin/cargo-nextest", "nextest", "run"][..],
        ] {
            let listed = listed_without_message_format(prefix, program);
            let mut expected = argv(&program[..2]);
            expected.extend(argv(&[
                "list",
                "--workspace",
                "--locked",
                "--profile",
                "ci",
                "-E",
                "all()",
                "--message-format",
                "json",
            ]));
            assert_eq!(listed, Some(expected), "{prefix:?} {program:?}");
        }
    }
}

#[test]
fn a_partitioned_run_lists_the_same_partition() {
    let run = argv(&[
        "cargo-nextest",
        "nextest",
        "run",
        "--archive-file",
        "nextest-archive.tar.zst",
        "--test-threads",
        "2",
        "--partition",
        "hash:2/4",
        "--no-fail-fast",
        "-E",
        "all()",
    ]);

    assert_eq!(
        list_argv(&run),
        Some(argv(&[
            "cargo-nextest",
            "nextest",
            "list",
            "--archive-file",
            "nextest-archive.tar.zst",
            "--partition",
            "hash:2/4",
            "-E",
            "all()",
            "--message-format",
            "json",
        ]))
    );
}
