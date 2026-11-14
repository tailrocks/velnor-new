// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoFacts, RustSelection, SelectionReason};
use velnor_rust::{DetectError, INPUTS_LOCK, Reason, dep_info_paths, detect, rerun_if_changed};

use crate::facts::{TestResult, lib, repo};
use crate::select::{assert_selects, changed, glob, name, path, some, two_roots, unknown};

#[test]
fn dep_info_and_build_script_output_become_the_paths_they_list() {
    let dep_info = "/w/target/debug/deps/app-0123456789abcdef.d: app/src/main.rs app/src/my\\ data.txt /r/serde/src/lib.rs\n\
                    \n\
                    /w/target/debug/deps/libapp-0123456789abcdef.rmeta: app/src/main.rs app/src/my\\ data.txt /r/serde/src/lib.rs app/src/../../shared/a.txt\n\
                    \n\
                    app/src/main.rs:\n\
                    app/src/my\\ data.txt:\n\
                    \n\
                    # env-dep:CARGO_PKG_NAME=app\n";
    assert_eq!(
        dep_info_paths(dep_info),
        [
            "app/src/main.rs",
            "app/src/my data.txt",
            "/r/serde/src/lib.rs",
            "app/src/../../shared/a.txt",
        ]
    );
    let output = "cargo:rerun-if-changed=build.rs\n\
                  cargo:rustc-env=LAB=lib\n\
                  cargo::rerun-if-changed=../shared/a b.txt\n\
                  cargo:rerun-if-changed=\n\
                  cargo:warning=cargo:rerun-if-changed=x\n";
    assert_eq!(rerun_if_changed(output), ["build.rs", "../shared/a b.txt"]);
}

fn locked(facts: &mut RepoFacts, entries: &[(&str, &str)]) {
    for (file, package) in entries {
        facts
            .compile_inputs
            .entry(path(file))
            .or_default()
            .insert(name(package));
    }
}

fn input(file: &str) -> SelectionReason {
    SelectionReason::Input(path(file))
}

#[test]
fn a_path_the_inputs_lock_lists_selects_its_packages_even_when_rules_ignore_it() {
    let none = RustSelection::None;
    let mut facts = two_roots();
    assert_selects(
        &facts,
        vec![(
            &["shared-data/greeting.txt"],
            vec![
                unknown("shared-data/greeting.txt"),
                unknown("shared-data/greeting.txt"),
            ],
        )],
    );
    locked(
        &mut facts,
        &[
            ("shared-data/greeting.txt", "app"),
            ("README.md", "app"),
            ("docs/schema", "cli"),
            ("app/src/schema.json", "tool"),
        ],
    );
    facts.config.non_inputs = vec![glob("docs/**")];
    assert_selects(
        &facts,
        vec![
            (
                &["shared-data/greeting.txt"],
                vec![
                    some(&[("app", input("shared-data/greeting.txt"))]),
                    none.clone(),
                ],
            ),
            (
                &["README.md"],
                vec![some(&[("app", input("README.md"))]), none.clone()],
            ),
            (
                &["docs/schema/v1/a.json"],
                vec![
                    none.clone(),
                    some(&[("cli", input("docs/schema/v1/a.json"))]),
                ],
            ),
            (&["docs/schema.md"], vec![none.clone(), none.clone()]),
            (
                &["app/src/schema.json"],
                vec![
                    some(&[
                        ("app", changed("app/src/schema.json")),
                        ("tool", input("app/src/schema.json")),
                    ]),
                    none.clone(),
                ],
            ),
        ],
    );
}

#[test]
fn the_inputs_lock_becomes_facts_and_a_bad_line_or_an_unknown_package_is_an_error() -> TestResult {
    let view = |lock: &str| {
        repo(
            vec![("Cargo.toml", vec![lib("app", "app/Cargo.toml")?])],
            &[(INPUTS_LOCK.as_str(), lock)],
        )
    };
    assert_eq!(
        detect(&view("README.md\tapp\nshared/a.txt\tapp\n")?)?.compile_inputs,
        BTreeMap::from([
            (path("README.md"), BTreeSet::from([name("app")])),
            (path("shared/a.txt"), BTreeSet::from([name("app")])),
        ])
    );
    for (lock, reason) in [
        ("README.md\tapp\nshared/a.txt app\n", Reason::LockLine(2)),
        ("../a.txt\tapp\n", Reason::LockLine(1)),
        (
            "README.md\tapplication\n",
            Reason::UnknownLockedPackage("application".to_owned()),
        ),
    ] {
        assert_eq!(
            detect(&view(lock)?),
            Err(DetectError::Invalid {
                path: INPUTS_LOCK,
                reason,
            }),
            "{lock}"
        );
    }
    Ok(())
}
