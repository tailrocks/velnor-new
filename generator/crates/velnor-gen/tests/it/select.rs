// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    AllTrigger, ChangeSet, DependencyKind, MemberDependency, PackageName, RepoFacts, RepoPath,
    RustCiConfig, RustMember, RustRoot, RustSelection, SelectionReason, ToolPins,
};
use velnor_rust::select;

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn name(text: &str) -> PackageName {
    PackageName::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn member(package: &str, directory: &str, dependencies: &[(&str, &str)]) -> RustMember {
    RustMember {
        name: name(package),
        manifest: path(&format!("{directory}/Cargo.toml")),
        directory: Some(path(directory)),
        dependencies: dependencies
            .iter()
            .map(|(package, directory)| MemberDependency {
                name: name(package),
                kind: DependencyKind::Normal,
                manifest: path(&format!("{directory}/Cargo.toml")),
            })
            .collect(),
        targets: Vec::new(),
    }
}

fn facts(roots: Vec<(&str, Vec<RustMember>)>) -> RepoFacts {
    RepoFacts {
        rust_roots: roots
            .into_iter()
            .map(|(manifest, members)| RustRoot {
                manifest: path(manifest),
                members,
                depends_on: Vec::new(),
            })
            .collect(),
        toolchain: None,
        tools: ToolPins {
            configs: Vec::new(),
            tools: BTreeMap::new(),
        },
        config: RustCiConfig::default(),
        helper: None,
    }
}

fn changed(file: &str) -> SelectionReason {
    SelectionReason::Changed(path(file))
}

fn depends_on(package: &str) -> SelectionReason {
    SelectionReason::DependsOn(name(package))
}

fn packages(entries: &[(&str, SelectionReason)]) -> BTreeMap<PackageName, SelectionReason> {
    entries
        .iter()
        .map(|(package, reason)| (name(package), reason.clone()))
        .collect()
}

fn some(entries: &[(&str, SelectionReason)]) -> RustSelection {
    RustSelection::Some(packages(entries))
}

fn most(entries: &[(&str, SelectionReason)]) -> RustSelection {
    RustSelection::All(AllTrigger::MostPackages(packages(entries)))
}

fn unknown(file: &str) -> RustSelection {
    RustSelection::All(AllTrigger::UnknownPath(path(file)))
}

fn assert_selects(facts: &RepoFacts, rows: Vec<(&[&str], Vec<RustSelection>)>) {
    for (files, expected) in rows {
        let changes = ChangeSet {
            paths: files.iter().map(|file| path(file)).collect(),
            base_files: BTreeMap::new(),
        };
        let expected: BTreeMap<RepoPath, RustSelection> = facts
            .rust_roots
            .iter()
            .map(|root| root.manifest.clone())
            .zip(expected)
            .collect();
        assert_eq!(select(facts, &changes), expected, "{files:?}");
    }
}

fn two_roots() -> RepoFacts {
    facts(vec![
        (
            "Cargo.toml",
            vec![
                member("app", "app", &[("lib", "lib")]),
                member("core", "core", &[]),
                member("lib", "lib", &[("core", "core")]),
                member("tool", "tool", &[("shared", "helper/shared")]),
            ],
        ),
        (
            "helper/Cargo.toml",
            vec![
                member("cli", "helper/cli", &[]),
                member("lint", "helper/lint", &[]),
                member("shared", "helper/shared", &[]),
            ],
        ),
    ])
}

#[test]
fn a_change_selects_its_package_and_every_package_that_depends_on_it_in_any_root() {
    let facts = two_roots();
    let none = RustSelection::None;
    assert_selects(
        &facts,
        vec![
            (
                &["app/src/main.rs"],
                vec![some(&[("app", changed("app/src/main.rs"))]), none.clone()],
            ),
            (
                &["lib/src/lib.rs"],
                vec![
                    some(&[
                        ("lib", changed("lib/src/lib.rs")),
                        ("app", depends_on("lib")),
                    ]),
                    none.clone(),
                ],
            ),
            (
                &["core/src/lib.rs"],
                vec![
                    most(&[
                        ("core", changed("core/src/lib.rs")),
                        ("lib", depends_on("core")),
                        ("app", depends_on("lib")),
                    ]),
                    none.clone(),
                ],
            ),
            (
                &["tool/src/main.rs"],
                vec![some(&[("tool", changed("tool/src/main.rs"))]), none.clone()],
            ),
            (
                &["unknown/file.bin"],
                vec![unknown("unknown/file.bin"), none.clone()],
            ),
            (&[], vec![none.clone(), none.clone()]),
            (
                &["helper/cli/src/main.rs"],
                vec![
                    none.clone(),
                    some(&[("cli", changed("helper/cli/src/main.rs"))]),
                ],
            ),
            (
                &["helper/shared/src/lib.rs"],
                vec![
                    some(&[("tool", depends_on("shared"))]),
                    some(&[("shared", changed("helper/shared/src/lib.rs"))]),
                ],
            ),
            (
                &["app/src/main.rs", "lib/src/lib.rs"],
                vec![
                    some(&[
                        ("app", changed("app/src/main.rs")),
                        ("lib", changed("lib/src/lib.rs")),
                    ]),
                    none.clone(),
                ],
            ),
        ],
    );
}

#[test]
fn a_file_selection_cannot_place_selects_everything_that_could_read_it() {
    let facts = facts(vec![
        (
            "a/Cargo.toml",
            vec![
                member("one", "a/one", &[("patched", "b/vendor/patched")]),
                member("two", "a/two", &[]),
                member("three", "a/three", &[]),
            ],
        ),
        (
            "b/Cargo.toml",
            vec![member("four", "b/four", &[]), member("five", "b/five", &[])],
        ),
    ]);
    assert_selects(
        &two_roots(),
        vec![
            (
                &["Cargo.lock"],
                vec![unknown("Cargo.lock"), RustSelection::None],
            ),
            (
                &["helper/README.md"],
                vec![
                    some(&[("tool", depends_on("shared"))]),
                    unknown("helper/README.md"),
                ],
            ),
        ],
    );
    assert_selects(
        &facts,
        vec![
            (
                &["docs/guide.md"],
                vec![unknown("docs/guide.md"), unknown("docs/guide.md")],
            ),
            (
                &["b/vendor/patched/src/lib.rs"],
                vec![
                    some(&[("one", depends_on("patched"))]),
                    unknown("b/vendor/patched/src/lib.rs"),
                ],
            ),
            (
                &["ab/one/src/lib.rs"],
                vec![unknown("ab/one/src/lib.rs"), unknown("ab/one/src/lib.rs")],
            ),
        ],
    );
}
