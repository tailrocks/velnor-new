// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const CI_PROFILE: &str = "[profile.ci]\nslow-timeout = { period = \"2s\", terminate-after = 1 }\n\n[profile.default]\nslow-timeout = \"60s\"\n";
const IMPLICIT_CI_PROFILE: &str =
    "[[profile.ci.overrides]]\nfilter = \"test(a)\"\nslow-timeout = \"60s\"\n";
const DEFAULT_PROFILE_ONLY: &str = "[profile.default]\nslow-timeout = \"60s\"\n";
const UNPARSEABLE: &str = "[profile.ci\n";

const RUN_WITH_CI: &str =
    "cargo-nextest nextest run --workspace --locked --profile ci --no-fail-fast";
const RUN_WITHOUT_CI: &str = "cargo-nextest nextest run --workspace --locked --no-fail-fast";
const BUILD_WITHOUT_CI: &str =
    "cargo-nextest nextest run --workspace --locked --ignore-default-filter --no-run";
const ARCHIVE_RUN_WITH_CI: &str = "--workspace-remap . --profile ci --no-fail-fast";
const ARCHIVE_WITHOUT_CI: &str = "cargo-nextest nextest archive --workspace --locked";

fn rendered(
    roots: &[&str],
    config: Option<&str>,
    nextest: &[(&str, &str)],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view(roots, config.map(str::as_bytes))?;
    for (path, text) in nextest {
        let path = RepoPath::new(*path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    workflow(&view)
}

fn ci_runs(text: &str) -> usize {
    text.matches("--profile ci").count()
}

#[test]
fn a_root_that_defines_profile_ci_runs_nextest_with_it() -> TestResult {
    let ci = rendered(
        &["Cargo.toml"],
        None,
        &[(".config/nextest.toml", CI_PROFILE)],
    )?;
    assert!(ci.contains(RUN_WITH_CI), "{ci}");
    assert!(ci.contains(BUILD_WITHOUT_CI), "{ci}");
    assert_eq!(ci_runs(&ci), 1, "{ci}");
    let implicit = rendered(
        &["Cargo.toml"],
        None,
        &[(".config/nextest.toml", IMPLICIT_CI_PROFILE)],
    )?;
    assert!(implicit.contains(RUN_WITH_CI), "{implicit}");
    Ok(())
}

#[test]
fn a_root_without_profile_ci_stays_profile_less() -> TestResult {
    for (name, config) in [
        ("default only", DEFAULT_PROFILE_ONLY),
        ("unparseable", UNPARSEABLE),
    ] {
        let text = rendered(&["Cargo.toml"], None, &[(".config/nextest.toml", config)])?;
        assert!(text.contains(RUN_WITHOUT_CI), "{name}\n{text}");
        assert_eq!(ci_runs(&text), 0, "{name}\n{text}");
    }
    let none = rendered(&["Cargo.toml"], None, &[])?;
    assert!(none.contains(RUN_WITHOUT_CI), "{none}");
    assert_eq!(ci_runs(&none), 0, "{none}");
    Ok(())
}

#[test]
fn the_archived_run_uses_the_ci_profile_and_the_archive_build_does_not() -> TestResult {
    let groups = rendered(
        &["Cargo.toml"],
        Some("shape = \"groups\"\n"),
        &[(".config/nextest.toml", CI_PROFILE)],
    )?;
    assert!(groups.contains(ARCHIVE_RUN_WITH_CI), "{groups}");
    assert!(groups.contains(ARCHIVE_WITHOUT_CI), "{groups}");
    assert_eq!(ci_runs(&groups), 1, "{groups}");
    Ok(())
}

#[test]
fn profile_detection_is_per_root() -> TestResult {
    let text = rendered(
        &["Cargo.toml", "lab/Cargo.toml"],
        None,
        &[("lab/.config/nextest.toml", CI_PROFILE)],
    )?;
    assert!(text.contains(RUN_WITH_CI), "{text}");
    assert!(text.contains(RUN_WITHOUT_CI), "{text}");
    assert_eq!(ci_runs(&text), 1, "{text}");
    Ok(())
}
