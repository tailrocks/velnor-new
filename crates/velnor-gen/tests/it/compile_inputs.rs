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
            .entry(Some(path(file)))
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

    let mut everywhere = two_roots();
    everywhere
        .compile_inputs
        .insert(None, BTreeSet::from([name("cli")]));
    assert_selects(
        &everywhere,
        vec![
            (
                &["CHANGELOG.md"],
                vec![none.clone(), some(&[("cli", input("CHANGELOG.md"))])],
            ),
            (
                &["unknown/file.bin"],
                vec![unknown("unknown/file.bin"), unknown("unknown/file.bin")],
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
        detect(&view(".\tapp\nREADME.md\tapp\nshared/a.txt\tapp\n")?)?.compile_inputs,
        BTreeMap::from([
            (None, BTreeSet::from([name("app")])),
            (Some(path("README.md")), BTreeSet::from([name("app")])),
            (Some(path("shared/a.txt")), BTreeSet::from([name("app")])),
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

fn embedding(files: &[(&str, &str)], check: &[(&str, &[&str])]) -> TestResult {
    embedding_with_links(&[], files, check)
}

fn embedding_with_links(
    links: &[&str],
    files: &[(&str, &str)],
    check: &[(&str, &[&str])],
) -> TestResult {
    let mut view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("app", "crates/app/Cargo.toml")?,
                lib("tool", "crates/tool/Cargo.toml")?,
                lib("reader", "crates/reader/Cargo.toml")?,
                lib("left", "crates/left/Cargo.toml")?,
            ],
        )],
        files,
    )?;
    for link in links {
        view.tracked_paths.insert(path(link));
        view.symlinks.insert(path(link));
    }
    let facts = detect(&view)?;
    let selected = |changed_file: &str| {
        let changes = velnor_model::ChangeSet {
            paths: BTreeSet::from([path(changed_file)]),
            base_files: BTreeMap::new(),
            head_files: BTreeMap::new(),
        };
        match velnor_rust::select(&facts, &changes).into_values().next() {
            Some(RustSelection::Some(map)) => map.keys().map(ToString::to_string).collect(),
            Some(RustSelection::All(_)) => vec!["*".to_owned()],
            _ => Vec::new(),
        }
    };
    for (file, expected) in check {
        assert_eq!(&selected(file), expected, "{file}");
    }
    Ok(())
}

#[test]
fn a_file_a_crate_embeds_selects_it_without_an_inputs_lock_line() -> TestResult {
    embedding(
        &[
            (
                "crates/app/src/lib.rs",
                "const A: &str = include_str!(\"../../../docs/guide.md\");\nconst H: &[u8] = include_bytes!(\"../../../shared/header.h\");",
            ),
            (
                "crates/tool/src/lib.rs",
                "const H: &str = include_str!(\"../../../shared/header.h\");",
            ),
            (
                "crates/reader/src/lib.rs",
                "const B: &str = include_str!(\"../../left/src/banner.txt\");",
            ),
            ("crates/left/src/lib.rs", ""),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("shared/header.h", &["app", "tool"]),
            ("crates/left/src/banner.txt", &["left", "reader"]),
            ("docs/other.md", &[]),
        ],
    )
}

#[test]
fn an_embedded_path_that_cannot_be_resolved_selects_its_crate_on_any_change() -> TestResult {
    embedding(
        &[
            (
                "crates/reader/src/lib.rs",
                "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/x.rs\"));",
            ),
            (
                "crates/app/src/lib.rs",
                "include_str!(\"../../../docs/guide.md\");",
            ),
        ],
        &[
            ("docs/guide.md", &["app", "reader"]),
            ("docs/other.md", &["reader"]),
        ],
    )
}

#[test]
fn a_crate_with_a_symlinked_source_is_selected_by_any_change_because_its_target_is_unread()
-> TestResult {
    embedding_with_links(
        &["crates/app/src/shared.rs"],
        &[
            ("crates/app/src/lib.rs", ""),
            ("crates/tool/src/lib.rs", ""),
            ("crates/reader/src/lib.rs", ""),
            ("crates/left/src/lib.rs", ""),
        ],
        &[("docs/shared.rs", &["app"]), ("docs/other.md", &["app"])],
    )
}
