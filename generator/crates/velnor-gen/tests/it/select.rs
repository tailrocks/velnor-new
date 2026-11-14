// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    AllTrigger, ChangeSet, DependencyKind, MemberDependency, PackageName, PathGlob, RepoFacts,
    RepoPath, RustCiConfig, RustMember, RustRoot, RustSelection, SelectionReason, ToolPins,
};
use velnor_rust::select;

pub(crate) fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn name(text: &str) -> PackageName {
    PackageName::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn member(package: &str, directory: &str, dependencies: &[(&str, &str)]) -> RustMember {
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

pub(crate) fn facts(roots: Vec<(&str, Vec<RustMember>)>) -> RepoFacts {
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
        compile_inputs: BTreeMap::new(),
        timings: BTreeMap::new(),
    }
}

pub(crate) fn changed(file: &str) -> SelectionReason {
    SelectionReason::Changed(path(file))
}

pub(crate) fn depends_on(package: &str) -> SelectionReason {
    SelectionReason::DependsOn(name(package))
}

fn packages(entries: &[(&str, SelectionReason)]) -> BTreeMap<PackageName, SelectionReason> {
    entries
        .iter()
        .map(|(package, reason)| (name(package), reason.clone()))
        .collect()
}

pub(crate) fn some(entries: &[(&str, SelectionReason)]) -> RustSelection {
    RustSelection::Some(packages(entries))
}

fn most(entries: &[(&str, SelectionReason)]) -> RustSelection {
    RustSelection::All(AllTrigger::MostPackages(packages(entries)))
}

pub(crate) fn unknown(file: &str) -> RustSelection {
    RustSelection::All(AllTrigger::UnknownPath(path(file)))
}

pub(crate) fn all(trigger: AllTrigger) -> RustSelection {
    RustSelection::All(trigger)
}

pub(crate) fn glob(text: &str) -> PathGlob {
    PathGlob::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn assert_selects(facts: &RepoFacts, rows: Vec<(&[&str], Vec<RustSelection>)>) {
    for (files, expected) in rows {
        let changes = ChangeSet {
            paths: files.iter().map(|file| path(file)).collect(),
            base_files: BTreeMap::new(),
            head_files: BTreeMap::new(),
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

pub(crate) fn two_roots() -> RepoFacts {
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
                vec![unknown("unknown/file.bin"), unknown("unknown/file.bin")],
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
        vec![(
            &["helper/README.md"],
            vec![unknown("helper/README.md"), unknown("helper/README.md")],
        )],
    );
    assert_selects(
        &facts,
        vec![
            (
                &["a/README.md"],
                vec![unknown("a/README.md"), unknown("a/README.md")],
            ),
            (
                &["b/vendor/patched/src/lib.rs"],
                vec![
                    unknown("b/vendor/patched/src/lib.rs"),
                    unknown("b/vendor/patched/src/lib.rs"),
                ],
            ),
            (
                &["b/four/src/lib.rs"],
                vec![
                    some(&[("one", depends_on("patched"))]),
                    some(&[("four", changed("b/four/src/lib.rs"))]),
                ],
            ),
            (
                &["a/one2/src/lib.rs"],
                vec![unknown("a/one2/src/lib.rs"), unknown("a/one2/src/lib.rs")],
            ),
        ],
    );
}

#[test]
fn a_package_at_the_root_of_its_workspace_claims_only_its_own_sources() {
    let top = RustMember {
        directory: None,
        manifest: path("Cargo.toml"),
        ..member("top", "top", &[])
    };
    let facts = facts(vec![
        (
            "Cargo.toml",
            vec![
                top,
                member("m1", "m1", &[]),
                member("m2", "m2", &[]),
                member("m3", "m3", &[]),
            ],
        ),
        (
            "inner/Cargo.toml",
            vec![member("i1", "inner/i1", &[]), member("i2", "inner/i2", &[])],
        ),
    ]);
    let none = RustSelection::None;
    let mut rows: Vec<(&[&str], Vec<RustSelection>)> = ["src/lib.rs", "build.rs", "tests/it.rs"]
        .iter()
        .map(|file| -> (&[&str], Vec<RustSelection>) {
            (
                std::slice::from_ref(file),
                vec![some(&[("top", changed(file))]), none.clone()],
            )
        })
        .collect();
    rows.extend([
        (
            &["Cargo.lock"][..],
            vec![
                all(AllTrigger::WorkspaceManifest),
                all(AllTrigger::WorkspaceManifest),
            ],
        ),
        (&["README.md"], vec![none.clone(), none.clone()]),
        (
            &["Cargo.toml"],
            vec![
                all(AllTrigger::WorkspaceManifest),
                all(AllTrigger::WorkspaceManifest),
            ],
        ),
        (
            &["inner/i1/src/lib.rs"],
            vec![
                none.clone(),
                some(&[("i1", changed("inner/i1/src/lib.rs"))]),
            ],
        ),
        (
            &["inner/README.md"],
            vec![unknown("inner/README.md"), unknown("inner/README.md")],
        ),
    ]);
    assert_selects(&facts, rows);
}
