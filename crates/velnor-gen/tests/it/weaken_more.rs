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

#[test]
fn autolib_false_a_test_runtool_and_an_override_platform_change_are_named() -> TestResult {
    let manifest = named(
        &[(MANIFEST, "[package]\nname = \"bar\"\nautolib = false\n")],
        false,
    )?;
    assert!(
        manifest.join("\n").contains("autolib = false"),
        "{manifest:?}"
    );
    let runtool = named(
        &[(
            ".cargo/config.toml",
            "[build]\nrustdocflags = [\"--test-runtool\", \"true\"]\n",
        )],
        false,
    )?;
    assert!(runtool.join("\n").contains("--test-runtool"), "{runtool:?}");
    let platform = |host: &str| {
        format!(
            "[[profile.default.overrides]]\nfilter = \"all()\"\nplatform = \"{host}\"\nretries = 2\n"
        )
    };
    let before = facts(&[(NEXTEST, &platform("cfg(windows)"))])?;
    assert_eq!(
        introduced_files(&before, &[NEXTEST], &[(NEXTEST, &platform("cfg(unix)"))])?,
        [NEXTEST]
    );
    Ok(())
}

#[test]
fn a_zero_retry_override_and_a_wrapper_in_an_inactive_profile_are_not_named() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[scripts.wrapper.w]\ncommand = \"true\"\n\n[profile.local]\nscripts = [{ filter = \"all()\", run-wrapper = \"w\" }]\n\n[[profile.default.overrides]]\nfilter = \"all()\"\nretries = 0\n",
        )],
        false,
    )?;
    assert_eq!(named, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_list_wrapper_that_could_hide_tests_from_discovery_is_named() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[scripts.wrapper.sub]\ncommand = \"head -1\"\n\n[[profile.default.scripts]]\nfilter = \"all()\"\nlist-wrapper = \"sub\"\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            ".config/nextest.toml: profile.default.scripts list-wrapper = \"sub\" command = \"head -1\" filter = \"all()\": lists tests through the wrapper sub, command \"head -1\""
        ]
    );
    Ok(())
}

#[test]
fn a_changed_mise_wrapper_env_is_new_and_defaults_stay_unnamed() -> TestResult {
    let mise = |env: &str| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\n{env}"
        )
    };
    let standard = "env = { MBX_CARGO_SHIM_MODE = \"1\" }\n";
    let after = facts(&[(
        "mise.toml",
        &mise("env = { MBX_CARGO_SHIM_MODE = \"1\", RUSTDOC = \"true\" }\n"),
    )])?;
    assert_eq!(
        introduced_files(&after, &["mise.toml"], &[("mise.toml", &mise(standard))])?,
        ["mise.toml"]
    );
    let build = named(
        &[(MANIFEST, "[package]\nname = \"bar\"\nbuild = false\n")],
        false,
    )?;
    assert!(build.join("\n").contains("build = false"), "{build:?}");
    let defaults = named(
        &[(
            MANIFEST,
            "[package]\nname = \"bar\"\n\n[[bench]]\nname = \"b\"\ntest = false\n\n[[example]]\nname = \"e\"\ntest = false\n",
        )],
        false,
    )?;
    assert_eq!(defaults, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_zero_count_retry_table_is_not_named_and_a_script_platform_change_is_new() -> TestResult {
    let named = named(
        &[(
            NEXTEST,
            "[profile.default]\nretries = { backoff = \"fixed\", count = 0 }\n",
        )],
        false,
    )?;
    assert_eq!(named, Vec::<String>::new());
    let wrapper = |host: &str| {
        format!(
            "[scripts.wrapper.skip]\ncommand = \"true\"\n\n[[profile.default.scripts]]\nfilter = \"all()\"\nplatform = \"{host}\"\nrun-wrapper = \"skip\"\n"
        )
    };
    let before = facts(&[(NEXTEST, &wrapper("cfg(windows)"))])?;
    assert_eq!(
        introduced_files(&before, &[NEXTEST], &[(NEXTEST, &wrapper("cfg(unix)"))])?,
        [NEXTEST]
    );
    Ok(())
}

#[test]
fn turning_the_cargo_deny_policy_off_or_narrowing_its_checks_is_new() -> TestResult {
    const CONFIG: &str = ".velnor/rust-ci.toml";
    let off = facts(&[(CONFIG, "[policy]\ndeny = false\n")])?;
    assert_eq!(introduced_files(&off, &[CONFIG], &[])?, [CONFIG]);
    assert_eq!(
        introduced_files(&off, &[CONFIG], &[(CONFIG, "[policy]\ndeny = false\n")])?,
        Vec::<String>::new()
    );
    let narrowed = facts(&[(CONFIG, "[policy]\ndeny_checks = [\"advisories\"]\n")])?;
    assert_eq!(introduced_files(&narrowed, &[CONFIG], &[])?, [CONFIG]);
    Ok(())
}
