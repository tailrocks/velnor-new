// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{InputOwner, RepoFacts, RustSelection, SelectionReason};
use velnor_rust::{DetectError, INPUTS_LOCK, Reason, dep_info_paths, detect, rerun_if_changed};

use crate::facts::{TestResult, lib, repo};
use crate::select::{assert_selects, changed, glob, name, named, path, some, two_roots, unknown};

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
            .insert(InputOwner::named(name(package)));
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
        .insert(None, BTreeSet::from([InputOwner::named(name("cli"))]));
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
            (None, BTreeSet::from([named("app")])),
            (Some(path("README.md")), BTreeSet::from([named("app")])),
            (Some(path("shared/a.txt")), BTreeSet::from([named("app")])),
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
    let view = repo(
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
fn a_file_an_included_file_embeds_selects_the_crate() -> TestResult {
    embedding(
        &[
            ("crates/app/src/lib.rs", "include!(\"gen.rs\");"),
            (
                "crates/app/src/gen.rs",
                "const A: &str = include_str!(\"../../../docs/guide.md\");\ninclude!(\"deep.rs\");",
            ),
            (
                "crates/app/src/deep.rs",
                "const B: &str = include_str!(\"../../../docs/deep.md\");",
            ),
            ("crates/tool/src/lib.rs", ""),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("docs/deep.md", &["app"]),
            ("docs/other.md", &[]),
        ],
    )
}

#[test]
fn an_included_file_the_view_cannot_read_fails_closed() -> TestResult {
    embedding(
        &[
            ("crates/app/src/lib.rs", "include!(\"body.inc\");"),
            ("crates/tool/src/lib.rs", ""),
        ],
        &[("docs/other.md", &["app"])],
    )
}

#[test]
fn a_comment_between_the_macro_and_its_bang_does_not_hide_it() -> TestResult {
    embedding(
        &[
            (
                "crates/app/src/lib.rs",
                "const A: &str = include_str/* generated */!(\"../../../docs/guide.md\");\nconst B: &str = include_str // x\n!(\"../../../docs/b.md\");",
            ),
            (
                "crates/tool/src/lib.rs",
                "// include_str!(\"../../../docs/commented.md\")\nconst S: &str = \"include_str!(\\\"../../../docs/quoted.md\\\")\";",
            ),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("docs/b.md", &["app"]),
            ("docs/commented.md", &[]),
            ("docs/quoted.md", &[]),
        ],
    )
}

#[test]
fn a_nested_package_is_not_charged_with_its_ancestors_scan() -> TestResult {
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("top", "Cargo.toml")?,
                lib("inner", "crates/inner/Cargo.toml")?,
            ],
        )],
        &[
            (
                "src/lib.rs",
                "const A: &str = include_str!(\"../docs/top.md\");",
            ),
            (
                "crates/inner/src/lib.rs",
                "const B: &str = include_str!(\"../../../docs/inner.md\");",
            ),
        ],
    )?;
    let facts = detect(&view)?;
    let owners = |file: &str| -> Option<Vec<String>> {
        facts
            .compile_inputs
            .get(&Some(path(file)))
            .map(|owners| owners.iter().map(|owner| owner.name.to_string()).collect())
    };
    assert_eq!(owners("docs/top.md"), Some(vec!["top".to_owned()]));
    assert_eq!(owners("docs/inner.md"), Some(vec!["inner".to_owned()]));
    assert!(
        !facts.compile_inputs.contains_key(&None),
        "{:?}",
        facts.compile_inputs
    );
    Ok(())
}

#[test]
fn a_target_source_outside_the_package_directory_attributes_its_includes() -> TestResult {
    let mut app = lib("app", "crates/app/Cargo.toml")?;
    for target in &mut app.targets {
        target.source = Some(path("shared/app_lib.rs"));
    }
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![app, lib("tool", "crates/tool/Cargo.toml")?],
        )],
        &[
            (
                "shared/app_lib.rs",
                "const A: &str = include_str!(\"../docs/guide.md\");\nmod more;",
            ),
            (
                "shared/more.rs",
                "const B: &str = include_str!(\"../docs/more.md\");",
            ),
            ("crates/tool/src/lib.rs", ""),
        ],
    )?;
    let facts = detect(&view)?;
    for file in ["docs/guide.md", "docs/more.md"] {
        let names: Vec<String> = facts
            .compile_inputs
            .get(&Some(path(file)))
            .into_iter()
            .flatten()
            .map(|owner| owner.name.to_string())
            .collect();
        assert_eq!(names, ["app"], "{file}");
    }
    Ok(())
}

#[test]
fn a_symlinked_rust_source_fails_closed_instead_of_being_skipped() -> TestResult {
    let mut view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("app", "crates/app/Cargo.toml")?,
                lib("tool", "crates/tool/Cargo.toml")?,
            ],
        )],
        &[("crates/tool/src/lib.rs", "")],
    )?;
    let linked = path("crates/app/src/lib.rs");
    view.tracked_paths.insert(linked.clone());
    view.symlinks.insert(linked);
    let facts = detect(&view)?;
    let app = InputOwner::at(name("app"), path("crates/app/Cargo.toml"));
    assert_eq!(
        facts.compile_inputs.get(&None),
        Some(&BTreeSet::from([app]))
    );
    Ok(())
}
