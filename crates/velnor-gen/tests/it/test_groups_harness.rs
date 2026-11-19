// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::TargetKind;
use velnor_rust::CONFIG_PATH;

use crate::facts::{TestResult, package, repo};
use crate::pipeline::{HELPER_LOCK, workflow};

const MANIFEST: &str = "[package]\nname = \"a\"\n[[test]]\nname = \"custom\"\nharness = false\n";
const NEXTEST_MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
const GROUP: &str = "[[test_groups]]\nname = 'db'\nfilter = 'package(a) and test(~db_)'\n";

#[test]
fn a_group_and_the_default_job_both_leave_out_the_harness_that_cargo_test_runs() -> TestResult {
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[
            ("a", TargetKind::Lib, true),
            ("custom", TargetKind::Test, true),
        ],
    )?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.81", "0.1.90", 1);
    let text = workflow(&repo(
        vec![("Cargo.toml", vec![a])],
        &[
            ("mise.toml", NEXTEST_MISE),
            ("a/Cargo.toml", MANIFEST),
            (CONFIG_PATH.as_str(), GROUP),
            (".velnor/rust-ci.lock", &lock),
        ],
    )?)?
    .replace("''", "'");
    let runs: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("nextest run --archive-file"))
        .collect();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(
        runs[0].contains(
            "-E \"(($TEST_FILTER) and not (package(a) and test(~db_))) - binary_id(=a::custom)\""
        ),
        "{}",
        runs[0]
    );
    assert!(
        runs[1].contains(
            "-E \"(($TEST_FILTER) and (package(a) and test(~db_))) - binary_id(=a::custom)\""
        ),
        "{}",
        runs[1]
    );
    assert!(text.contains(
        "cargo test -p a --locked --message-format=json-render-diagnostics --test=custom"
    ));
    Ok(())
}
