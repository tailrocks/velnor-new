// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::weaken_scope::{facts, introduced_files, named};

const NEXTEST: &str = ".config/nextest.toml";
const MANIFEST: &str = "crates/foo/Cargo.toml";

fn timeout(period: &str) -> String {
    format!(
        "[profile.default]\nslow-timeout = {{ period = \"{period}\", terminate-after = 1, on-timeout = \"pass\" }}\n"
    )
}

#[test]
fn lowering_a_passing_timeout_period_or_changing_flaky_result_is_new() -> TestResult {
    let before = facts(&[(NEXTEST, &timeout("60s"))])?;
    assert_eq!(
        introduced_files(&before, &[NEXTEST], &[(NEXTEST, &timeout("1s"))])?,
        [NEXTEST]
    );
    let retries =
        |result: &str| format!("[profile.default]\nretries = 2\nflaky-result = \"{result}\"\n");
    let before = facts(&[(NEXTEST, &retries("pass"))])?;
    assert_eq!(
        introduced_files(&before, &[NEXTEST], &[(NEXTEST, &retries("fail"))])?,
        [NEXTEST]
    );
    Ok(())
}

#[test]
fn profile_wide_run_extra_args_are_named() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[profile.default]\nrun-extra-args = [\"--ignored\"]\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            ".config/nextest.toml: profile.default run-extra-args = [\"--ignored\"]: passes extra arguments to every test binary in the profile"
        ]
    );
    Ok(())
}

#[test]
fn a_manifest_that_drops_docs_discovery_harness_or_a_default_off_feature_is_named() -> TestResult {
    let named = named(
        &[(
            MANIFEST,
            "[package]\nname = \"bar\"\nautoexamples = false\nautobenches = false\nautobins = false\n\n[features]\ndefault = [\"on\"]\non = []\noff = []\n\n[lib]\ndoc = false\n\n[[test]]\nname = \"custom\"\nharness = false\nrequired-features = [\"on\", \"off\"]\n\n[[test]]\nname = \"fine\"\nrequired-features = [\"on\"]\n",
        )],
        false,
    )?;
    let joined = named.join("\n");
    for setting in [
        "autoexamples = false",
        "autobenches = false",
        "autobins = false",
        "[lib] doc = false",
        "[[test]] name = \"custom\" harness = false",
        "[[test]] name = \"custom\" required-features = [\"off\"]",
    ] {
        assert!(joined.contains(setting), "{setting}: {joined}");
    }
    assert!(!joined.contains("name = \"fine\""), "{joined}");
    Ok(())
}
