// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::weaken_scope::{facts, introduced_files, named};

const NEXTEST: &str = ".config/nextest.toml";

#[test]
fn profile_retries_a_passing_slow_timeout_and_extra_run_args_are_named() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[profile.ci]\nretries = 2\nslow-timeout = { period = \"60s\", on-timeout = \"pass\" }\n\n[profile.default]\nretries = 0\nslow-timeout = \"60s\"\n\n[profile.other]\nretries = 5\n\n[[profile.default.overrides]]\nfilter = \"test(a)\"\nrun-extra-args = [\"--skip\", \"a\"]\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            ".config/nextest.toml: profile.ci retries = 2: retries every test in the profile, so a flaky failure passes",
            ".config/nextest.toml: profile.ci slow-timeout = { on-timeout = \"pass\", period = \"60s\" }: counts a test that times out as passed",
            ".config/nextest.toml: profile.default.overrides filter = \"test(a)\" run-extra-args = [\"--skip\", \"a\"]: passes extra arguments to the test binaries matching \"test(a)\"",
        ]
    );
    Ok(())
}

#[test]
fn swapping_the_command_of_a_run_wrapper_is_new() -> TestResult {
    let wrapper = |command: &str| {
        format!(
            "[scripts.wrapper.skip]\ncommand = \"{command}\"\n\n[[profile.default.scripts]]\nfilter = \"all()\"\nrun-wrapper = \"skip\"\n"
        )
    };
    let facts = facts(&[(NEXTEST, &wrapper("exit 0"))])?;
    assert_eq!(
        introduced_files(&facts, &[NEXTEST], &[(NEXTEST, &wrapper("env"))])?,
        [NEXTEST]
    );
    assert_eq!(
        introduced_files(&facts, &[NEXTEST], &[(NEXTEST, &wrapper("exit 0"))])?,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn two_unnamed_test_targets_with_tests_off_are_both_new() -> TestResult {
    let manifest = |paths: &[&str]| {
        let targets: Vec<String> = paths
            .iter()
            .map(|path| format!("\n[[test]]\npath = \"{path}\"\ntest = false\n"))
            .collect();
        format!("[package]\nname = \"bar\"\n{}", targets.concat())
    };
    let manifest_path = "crates/foo/Cargo.toml";
    let facts = facts(&[(manifest_path, &manifest(&["a.rs", "b.rs"]))])?;
    assert_eq!(facts.weakenings.len(), 2, "{:?}", facts.weakenings);
    assert_eq!(
        introduced_files(
            &facts,
            &[manifest_path],
            &[(manifest_path, &manifest(&["a.rs"]))]
        )?,
        [manifest_path]
    );
    Ok(())
}

#[test]
fn two_test_targets_with_neither_name_nor_path_are_named_under_distinct_keys() -> TestResult {
    let named = named(
        &[(
            "crates/foo/Cargo.toml",
            "[package]\nname = \"bar\"\n\n[[test]]\ntest = false\n\n[[test]]\ntest = false\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            "crates/foo/Cargo.toml: [[test]] name = \"unnamed #0\" test = false: leaves the test target unnamed #0 of bar out of the test run",
            "crates/foo/Cargo.toml: [[test]] name = \"unnamed #1\" test = false: leaves the test target unnamed #1 of bar out of the test run",
        ]
    );
    Ok(())
}

#[test]
fn a_test_target_with_only_a_path_is_named_by_its_path() -> TestResult {
    let named = named(
        &[(
            "crates/foo/Cargo.toml",
            "[package]\nname = \"bar\"\n\n[[test]]\npath = \"tests/x.rs\"\ntest = false\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            "crates/foo/Cargo.toml: [[test]] name = \"tests/x.rs\" test = false: leaves the test target tests/x.rs of bar out of the test run"
        ]
    );
    Ok(())
}

#[test]
fn an_override_that_passes_a_slow_timeout_and_a_table_form_profile_retry_are_named() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[[profile.default.overrides]]\nfilter = \"test(slow)\"\nslow-timeout = { period = \"1s\", on-timeout = \"pass\" }\n\n[profile.default]\nretries = { backoff = \"fixed\", count = 2 }\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            ".config/nextest.toml: profile.default retries = { backoff = \"fixed\", count = 2 }: retries every test in the profile, so a flaky failure passes",
            ".config/nextest.toml: profile.default.overrides filter = \"test(slow)\" slow-timeout = { on-timeout = \"pass\", period = \"1s\" }: counts a test matching \"test(slow)\" that times out as passed",
        ]
    );
    Ok(())
}
