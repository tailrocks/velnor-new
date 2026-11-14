// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{AllTrigger, InputRule, RepoFacts, RustSelection, SelectionReason};

use crate::select::{
    all, assert_selects, changed, depends_on, facts, glob, member, name, path, some, two_roots,
    unknown,
};

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
        ("Cargo.lock", AllTrigger::WorkspaceManifest),
        (
            ".velnor/rust-ci.toml",
            AllTrigger::Global(path(".velnor/rust-ci.toml")),
        ),
        (
            ".velnor/rust-inputs.lock",
            AllTrigger::Global(path(".velnor/rust-inputs.lock")),
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
