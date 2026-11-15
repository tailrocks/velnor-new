// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{ChangeSet, RepoFacts};
use velnor_rust::{detect, introduced};

use crate::facts::{TestResult, lib, path, repo};

const DOCTESTS_OFF: &str = "[lib]\ndoctest = false\n";
const RUNNER: &str = "[target.x86_64-unknown-linux-gnu]\nrunner = \"true\"\n";

pub(crate) fn named(
    files: &[(&str, &str)],
    untracked_roots: bool,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut workspaces = vec![("Cargo.toml", vec![lib("foo", "crates/foo/Cargo.toml")?])];
    if untracked_roots {
        workspaces.push(("tools/Cargo.toml", vec![lib("t", "tools/t/Cargo.toml")?]));
    }
    let mut view = repo(workspaces, files)?;
    for (file, _) in files {
        view.tracked_paths.insert(path(file)?);
    }
    Ok(detect(&view)?
        .weakenings
        .iter()
        .map(ToString::to_string)
        .collect())
}

pub(crate) fn introduced_files(
    facts: &RepoFacts,
    changed: &[&str],
    base: &[(&str, &str)],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let change_set = ChangeSet {
        paths: changed
            .iter()
            .map(|p| path(p))
            .collect::<Result<BTreeSet<_>, _>>()?,
        base_files: base
            .iter()
            .map(|(p, t)| Ok((path(p)?, (*t).to_owned())))
            .collect::<Result<BTreeMap<_, _>, Box<dyn std::error::Error>>>()?,
        head_files: BTreeMap::new(),
    };
    Ok(introduced(facts, &change_set)
        .iter()
        .map(|w| w.file.to_string())
        .collect())
}

pub(crate) fn facts(files: &[(&str, &str)]) -> Result<RepoFacts, Box<dyn std::error::Error>> {
    let mut view = repo(
        vec![("Cargo.toml", vec![lib("bar", "crates/foo/Cargo.toml")?])],
        files,
    )?;
    for (file, _) in files {
        view.tracked_paths.insert(path(file)?);
    }
    Ok(detect(&view)?)
}

#[test]
fn a_weakening_in_a_manifest_or_config_no_check_reads_is_not_named() -> TestResult {
    let named = named(
        &[
            (".velnor/rust-ci.toml", "exclude = [\"tools/Cargo.toml\"]\n"),
            (
                "tools/t/Cargo.toml",
                &format!("[package]\nname = \"t\"\n\n{DOCTESTS_OFF}"),
            ),
            (
                "vendor/x/Cargo.toml",
                &format!("[package]\nname = \"x\"\n\n{DOCTESTS_OFF}"),
            ),
            ("crates/foo/.cargo/config.toml", RUNNER),
            (
                "crates/foo/.config/nextest.toml",
                "[profile.default]\ndefault-filter = \"none()\"\n",
            ),
            ("tools/.cargo/config.toml", RUNNER),
        ],
        true,
    )?;
    assert_eq!(
        named,
        [
            ".velnor/rust-ci.toml: exclude = \"tools/Cargo.toml\": leaves tools/Cargo.toml out of the checks"
        ]
    );
    Ok(())
}

#[test]
fn a_member_manifest_and_a_config_above_a_root_are_still_named() -> TestResult {
    let named = named(
        &[
            (
                "crates/foo/Cargo.toml",
                &format!("[package]\nname = \"foo\"\n\n{DOCTESTS_OFF}"),
            ),
            (".cargo/config.toml", RUNNER),
        ],
        false,
    )?;
    assert_eq!(named.len(), 2, "{named:?}");
    Ok(())
}

#[test]
fn renaming_a_crate_with_doctests_already_off_is_not_new() -> TestResult {
    let renamed = format!("[package]\nname = \"bar\"\n\n{DOCTESTS_OFF}");
    let facts = facts(&[("crates/foo/Cargo.toml", &renamed)])?;
    let base = format!("[package]\nname = \"foo\"\n\n{DOCTESTS_OFF}");
    assert_eq!(
        introduced_files(
            &facts,
            &["crates/foo/Cargo.toml"],
            &[("crates/foo/Cargo.toml", &base)]
        )?,
        Vec::<String>::new()
    );
    assert_eq!(
        introduced_files(&facts, &["crates/foo/Cargo.toml"], &[])?,
        ["crates/foo/Cargo.toml"]
    );
    Ok(())
}

#[test]
fn a_broadened_wrapper_filter_or_rustdoc_argument_is_new() -> TestResult {
    let wrapper = |filter: &str| {
        format!(
            "[scripts.wrapper.skip]\ncommand = \"true\"\n\n[[profile.default.scripts]]\nfilter = \"{filter}\"\nrun-wrapper = \"skip\"\n"
        )
    };
    let flags = |arg: &str| format!("[build]\nrustdocflags = [\"--test-args\", \"{arg}\"]\n");
    let facts = facts(&[
        (".config/nextest.toml", &wrapper("all()")),
        (".cargo/config.toml", &flags("b")),
    ])?;
    let changed = [".config/nextest.toml", ".cargo/config.toml"];
    assert_eq!(
        introduced_files(
            &facts,
            &changed,
            &[
                (".config/nextest.toml", &wrapper("package(foo)")),
                (".cargo/config.toml", &flags("a")),
            ]
        )?,
        [".cargo/config.toml", ".config/nextest.toml"]
    );
    Ok(())
}

#[test]
fn a_nextest_override_that_retries_tests_is_named() -> TestResult {
    let named = named(
        &[(
            ".config/nextest.toml",
            "[[profile.default.overrides]]\nfilter = \"test(slow)\"\nretries = 3\n\n[[profile.default.overrides]]\nfilter = \"test(big)\"\nthreads-required = 2\n",
        )],
        false,
    )?;
    assert_eq!(
        named,
        [
            ".config/nextest.toml: profile.default.overrides filter = \"test(slow)\" retries = 3: retries the tests matching \"test(slow)\", so a flaky failure there passes"
        ]
    );
    Ok(())
}

#[test]
fn a_cargo_config_in_a_roots_folder_or_above_it_is_named_but_not_a_sibling() -> TestResult {
    let mut view = repo(
        vec![("sub/Cargo.toml", vec![lib("s", "sub/s/Cargo.toml")?])],
        &[
            ("sub/.cargo/config.toml", RUNNER),
            (".cargo/config.toml", RUNNER),
            ("other/.cargo/config.toml", RUNNER),
        ],
    )?;
    for file in [
        "sub/.cargo/config.toml",
        ".cargo/config.toml",
        "other/.cargo/config.toml",
    ] {
        view.tracked_paths.insert(path(file)?);
    }
    let files: Vec<String> = detect(&view)?
        .weakenings
        .iter()
        .map(|w| w.file.to_string())
        .collect();
    assert_eq!(files, [".cargo/config.toml", "sub/.cargo/config.toml"]);
    Ok(())
}

fn cargo_config_named(
    root: &str,
    member: &str,
    config: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut view = repo(vec![(root, vec![lib("r", member)?])], &[(config, RUNNER)])?;
    view.tracked_paths.insert(path(config)?);
    Ok(detect(&view)?
        .weakenings
        .iter()
        .map(|w| w.file.to_string())
        .collect())
}

#[test]
fn a_cargo_config_above_a_root_is_named_and_one_sharing_only_a_name_prefix_is_not() -> TestResult {
    assert_eq!(
        cargo_config_named(
            "sub/inner/Cargo.toml",
            "sub/inner/r/Cargo.toml",
            "sub/.cargo/config.toml"
        )?,
        ["sub/.cargo/config.toml"]
    );
    assert_eq!(
        cargo_config_named(
            "sub/inner2/Cargo.toml",
            "sub/inner2/r/Cargo.toml",
            "sub/inner/.cargo/config.toml"
        )?,
        Vec::<String>::new()
    );
    assert_eq!(
        cargo_config_named(
            "sub/inner/Cargo.toml",
            "sub/inner/r/Cargo.toml",
            "sub/in/.cargo/config.toml"
        )?,
        Vec::<String>::new()
    );
    Ok(())
}
