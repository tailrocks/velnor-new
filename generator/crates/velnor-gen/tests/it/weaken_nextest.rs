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
            ".config/nextest.toml: profile.ci slow-timeout on-timeout = \"pass\": counts a test that times out as passed",
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
