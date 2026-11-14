// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    AllTrigger, ChangeSet, DependencyKind, InputRule, MemberDependency, PackageName, PathGlob,
    RepoFacts, RepoPath, RustCiConfig, RustMember, RustRoot, RustSelection, SelectionReason,
    ToolPins,
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

fn all(trigger: AllTrigger) -> RustSelection {
    RustSelection::All(trigger)
}

fn glob(text: &str) -> PathGlob {
    PathGlob::new(text).unwrap_or_else(|e| unreachable!("{e}"))
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
                all(AllTrigger::Global(path("Cargo.lock"))),
                all(AllTrigger::Global(path("Cargo.lock"))),
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

type Row = (Vec<&'static str>, Vec<RustSelection>);

fn everywhere(files: &[&'static str], selection: impl Fn(&str) -> RustSelection) -> Vec<Row> {
    files
        .iter()
        .map(|file| (vec![*file], vec![selection(file), selection(file)]))
        .collect()
}

fn assert_rows(facts: &RepoFacts, rows: Vec<Row>) {
    for (files, expected) in rows {
        assert_selects(facts, vec![(files.as_slice(), expected)]);
    }
}

#[test]
fn a_global_path_outside_every_package_selects_everything_with_an_empty_config() {
    let facts = two_roots();
    let mut rows = Vec::new();
    for (file, trigger) in [
        (
            "rust-toolchain.toml",
            AllTrigger::ToolchainFile(path("rust-toolchain.toml")),
        ),
        (
            "rust-toolchain",
            AllTrigger::ToolchainFile(path("rust-toolchain")),
        ),
        (
            "helper/rust-toolchain.toml",
            AllTrigger::ToolchainFile(path("helper/rust-toolchain.toml")),
        ),
        (
            ".cargo/config.toml",
            AllTrigger::CargoConfig(path(".cargo/config.toml")),
        ),
        (
            ".cargo/config",
            AllTrigger::CargoConfig(path(".cargo/config")),
        ),
        (
            "helper/.cargo/config.toml",
            AllTrigger::CargoConfig(path("helper/.cargo/config.toml")),
        ),
        ("Cargo.toml", AllTrigger::WorkspaceManifest),
        ("helper/Cargo.toml", AllTrigger::WorkspaceManifest),
        ("docs/mise.toml", AllTrigger::Global(path("docs/mise.toml"))),
        (
            "docs/rust-toolchain.toml",
            AllTrigger::ToolchainFile(path("docs/rust-toolchain.toml")),
        ),
        (
            "docs/Cargo.lock",
            AllTrigger::Global(path("docs/Cargo.lock")),
        ),
        (
            "docs/.cargo/config.toml",
            AllTrigger::CargoConfig(path("docs/.cargo/config.toml")),
        ),
        ("mise.toml", AllTrigger::Global(path("mise.toml"))),
        ("mise.lock", AllTrigger::Global(path("mise.lock"))),
        ("Cargo.lock", AllTrigger::Global(path("Cargo.lock"))),
        (
            ".velnor/rust-ci.toml",
            AllTrigger::Global(path(".velnor/rust-ci.toml")),
        ),
        (
            ".github/workflows/rust-ci.yml",
            AllTrigger::GeneratedWorkflow,
        ),
    ] {
        rows.push((vec![file], vec![all(trigger.clone()), all(trigger)]));
    }
    assert_rows(&facts, rows);
}

#[test]
fn docs_and_licence_paths_select_nothing_and_look_alikes_select_everything() {
    let facts = two_roots();
    let mut rows = Vec::new();
    rows.extend(everywhere(
        &[
            "docs/guide.md",
            "docs/img/overview.png",
            "LICENSE",
            "LICENSE-MIT",
            "LICENSES/MIT.txt",
            "NOTICE",
            "CODEOWNERS",
            ".github/ISSUE_TEMPLATE/bug.yml",
            ".github/PULL_REQUEST_TEMPLATE.md",
            ".github/PULL_REQUEST_TEMPLATE/release.md",
            "README.md",
            "CHANGELOG.md",
        ],
        |_| RustSelection::None,
    ));
    rows.extend(everywhere(
        &[
            "helper/README.md",
            "helper/docs/guide.md",
            "data/docs/guide.md",
            "README.txt",
            "NOTICE.txt",
            "licenses/MIT.txt",
            ".github/CODEOWNERS",
            ".github/workflows/ci.yml",
            ".github/dependabot.yml",
            "scripts/build.sh",
            "build.rs",
            "mise/tasks.toml",
        ],
        unknown,
    ));
    assert_rows(&facts, rows);
}

#[test]
fn a_docs_looking_file_that_a_package_reads_still_selects_that_package() {
    let none = RustSelection::None;
    assert_rows(
        &two_roots(),
        vec![
            (
                vec!["app/README.md"],
                vec![some(&[("app", changed("app/README.md"))]), none.clone()],
            ),
            (
                vec!["app/docs/guide.md"],
                vec![some(&[("app", changed("app/docs/guide.md"))]), none.clone()],
            ),
            (
                vec!["helper/cli/LICENSE"],
                vec![
                    none.clone(),
                    some(&[("cli", changed("helper/cli/LICENSE"))]),
                ],
            ),
        ],
    );

    let mut mapped = two_roots();
    mapped.config.inputs = vec![
        InputRule {
            glob: glob("docs/api/*"),
            packages: vec![name("app")],
        },
        InputRule {
            glob: glob("LICENSE"),
            packages: vec![name("shared")],
        },
    ];
    assert_rows(
        &mapped,
        vec![
            (
                vec!["docs/api/openapi.yml"],
                vec![
                    some(&[("app", SelectionReason::Input(path("docs/api/openapi.yml")))]),
                    none.clone(),
                ],
            ),
            (
                vec!["LICENSE"],
                vec![
                    some(&[("tool", depends_on("shared"))]),
                    some(&[("shared", SelectionReason::Input(path("LICENSE")))]),
                ],
            ),
            (vec!["docs/guide.md"], vec![none.clone(), none.clone()]),
        ],
    );

    let mut shared = two_roots();
    shared.config.inputs = vec![
        InputRule {
            glob: glob("schema/*"),
            packages: vec![name("app")],
        },
        InputRule {
            glob: glob("schema/a.json"),
            packages: vec![name("cli")],
        },
    ];
    assert_rows(
        &shared,
        vec![(
            vec!["schema/a.json"],
            vec![
                some(&[("app", SelectionReason::Input(path("schema/a.json")))]),
                some(&[("cli", SelectionReason::Input(path("schema/a.json")))]),
            ],
        )],
    );

    let nested = facts(vec![
        ("Cargo.toml", vec![member("app", "app", &[])]),
        (
            "docs/Cargo.toml",
            vec![
                member("site", "docs/site", &[]),
                member("book", "docs/book", &[]),
                member("api", "docs/api", &[]),
            ],
        ),
    ]);
    assert_rows(
        &nested,
        vec![
            (
                vec!["docs/guide.md"],
                vec![unknown("docs/guide.md"), unknown("docs/guide.md")],
            ),
            (
                vec!["docs/site/src/lib.rs"],
                vec![
                    none.clone(),
                    some(&[("site", changed("docs/site/src/lib.rs"))]),
                ],
            ),
        ],
    );

    let vendored = facts(vec![(
        "Cargo.toml",
        vec![member("app", "app", &[("patched", "docs/vendor/patched")])],
    )]);
    assert_rows(
        &vendored,
        vec![(
            vec!["docs/vendor/patched/README.md"],
            vec![unknown("docs/vendor/patched/README.md")],
        )],
    );
}

#[test]
fn user_rules_win_and_default_ignores_can_be_turned_off() {
    let none = RustSelection::None;
    let mut ruled = two_roots();
    ruled.config.non_inputs = vec![
        glob("data/**"),
        glob("scripts/gen.sh"),
        glob("scripts/*/notes.txt"),
        glob(".cargo/config.toml"),
    ];
    assert_rows(
        &ruled,
        vec![
            (vec!["data/x/y.bin"], vec![none.clone(), none.clone()]),
            (vec!["scripts/gen.sh"], vec![none.clone(), none.clone()]),
            (
                vec!["scripts/gen.sh/out.rs"],
                vec![
                    unknown("scripts/gen.sh/out.rs"),
                    unknown("scripts/gen.sh/out.rs"),
                ],
            ),
            (vec!["data"], vec![unknown("data"), unknown("data")]),
            (
                vec!["scripts/a/notes.txt"],
                vec![none.clone(), none.clone()],
            ),
            (vec![".cargo/config.toml"], vec![none.clone(), none.clone()]),
            (
                vec!["scripts/a/run.sh"],
                vec![unknown("scripts/a/run.sh"), unknown("scripts/a/run.sh")],
            ),
            (
                vec!["scripts/a/b/notes.txt"],
                vec![
                    unknown("scripts/a/b/notes.txt"),
                    unknown("scripts/a/b/notes.txt"),
                ],
            ),
        ],
    );

    let mut off = two_roots();
    off.config.default_ignores = false;
    assert_rows(
        &off,
        vec![
            (
                vec!["README.md"],
                vec![unknown("README.md"), unknown("README.md")],
            ),
            (
                vec!["docs/guide.md"],
                vec![unknown("docs/guide.md"), unknown("docs/guide.md")],
            ),
            (
                vec!["rust-toolchain.toml"],
                vec![
                    all(AllTrigger::ToolchainFile(path("rust-toolchain.toml"))),
                    all(AllTrigger::ToolchainFile(path("rust-toolchain.toml"))),
                ],
            ),
        ],
    );
}
