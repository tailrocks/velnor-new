// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    AllTrigger, ChangeSet, InputRule, RepoFacts, RepoPath, RustSelection, SelectionReason,
};
use velnor_rust::select;

use crate::lock_affected::{Pkg, krate, member as locked_member, render, replaced, workspace};
use crate::select::{all, depends_on, facts, glob, member, name, path, some};

fn two_workspaces() -> RepoFacts {
    facts(vec![
        (
            "Cargo.toml",
            vec![
                member("app", "app", &[("lib", "lib")]),
                member("core", "core", &[]),
                member("lib", "lib", &[("core", "core")]),
                member("tool", "tool", &[]),
            ],
        ),
        (
            "helper/Cargo.toml",
            vec![
                member("cli", "helper/cli", &[("tool", "tool")]),
                member("lint", "helper/lint", &[]),
                member("shared", "helper/shared", &[]),
            ],
        ),
    ])
}

fn lock(packages: &[Pkg]) -> String {
    render(4, packages, "")
}

fn change(files: &[(&str, &str, &str)]) -> ChangeSet {
    let text = |index: usize| -> BTreeMap<RepoPath, String> {
        files
            .iter()
            .map(|file| {
                let text = [file.1, file.2][index];
                (path(file.0), text.to_owned())
            })
            .collect()
    };
    ChangeSet {
        paths: files.iter().map(|file| path(file.0)).collect(),
        base_files: text(0),
        head_files: text(1),
    }
}

fn lock_reason(package: &str) -> SelectionReason {
    SelectionReason::Lock(package.to_owned())
}

fn selected(facts: &RepoFacts, changes: &ChangeSet) -> Vec<RustSelection> {
    select(facts, changes).into_values().collect()
}

#[test]
fn a_lockfile_change_selects_the_members_that_reach_a_changed_crate_in_its_own_root_and_their_dependents()
 {
    let facts = two_workspaces();
    let base = workspace();
    let before = lock(&base);
    let fnv = lock(&replaced(&base, krate("fnv", "1.0.6", &[])));
    let serde = lock(&replaced(&base, krate("serde", "1.0.2", &["serde_derive"])));
    assert_eq!(
        selected(&facts, &change(&[("Cargo.lock", &before, &fnv)])),
        vec![
            some(&[("tool", lock_reason("fnv"))]),
            some(&[("cli", depends_on("tool"))]),
        ]
    );
    let core = RustSelection::All(AllTrigger::MostPackages(
        [
            (name("app"), lock_reason("serde")),
            (name("core"), lock_reason("serde")),
            (name("lib"), lock_reason("serde")),
        ]
        .into(),
    ));
    assert_eq!(
        selected(&facts, &change(&[("Cargo.lock", &before, &serde)])),
        vec![core, RustSelection::None]
    );
    let helper_lock = |fnv: &str| {
        lock(&[
            locked_member("cli", &["fnv"]),
            locked_member("lint", &[]),
            locked_member("shared", &[]),
            krate("fnv", fnv, &[]),
        ])
    };
    let (helper, helper_bumped) = (helper_lock("1.0.5"), helper_lock("1.0.6"));
    assert_eq!(
        selected(
            &facts,
            &change(&[("helper/Cargo.lock", &helper, &helper_bumped)])
        ),
        vec![RustSelection::None, some(&[("cli", lock_reason("fnv"))])]
    );
    assert_eq!(
        selected(&facts, &change(&[("Cargo.lock", &before, &before)])),
        vec![RustSelection::None, RustSelection::None]
    );
}

#[test]
fn a_lockfile_change_without_a_usable_base_selects_everything() {
    let facts = two_workspaces();
    let base = workspace();
    let before = lock(&base);
    let fnv = lock(&replaced(&base, krate("fnv", "1.0.6", &[])));
    let everything = |trigger: AllTrigger| vec![all(trigger.clone()), all(trigger)];
    let mut no_base = change(&[("Cargo.lock", &before, &fnv)]);
    no_base.base_files.clear();
    let mut no_head = change(&[("Cargo.lock", &before, &fnv)]);
    no_head.head_files.clear();
    let unparsable = change(&[("Cargo.lock", "not toml [", &fnv)]);
    let removed = change(&[("Cargo.lock", &before, &lock(&base[1..]))]);
    for changes in [no_base, no_head, unparsable, removed] {
        assert_eq!(
            selected(&facts, &changes),
            everything(AllTrigger::WorkspaceManifest),
            "{changes:?}"
        );
    }
    assert_eq!(
        selected(&facts, &change(&[("docs/Cargo.lock", &before, &fnv)])),
        everything(AllTrigger::Global(path("docs/Cargo.lock")))
    );
}

fn manifest(dependencies: &str, extra: &str) -> String {
    format!(
        "[workspace]\nmembers = [\"app\", \"core\"]\n{extra}\n[workspace.dependencies]\n{dependencies}"
    )
}

#[test]
fn a_root_manifest_changing_only_dependency_versions_leaves_the_decision_to_the_lockfile() {
    let facts = two_workspaces();
    let base = manifest(
        "serde = \"1\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
        "",
    );
    let exempt = [
        manifest(
            "serde = \"2\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
            "",
        ),
        manifest(
            "serde = \"1\"\nfnv = { version = \"1.0.6\", features = [\"std\"] }\n",
            "",
        ),
        manifest(
            "serde   =   \"1\"  # same\nfnv = { features = [\"std\"], version = \"1.0.5\" }\n",
            "",
        ),
    ];
    let locks = (
        lock(&workspace()),
        lock(&replaced(&workspace(), krate("fnv", "1.0.6", &[]))),
    );
    for head in &exempt {
        assert_eq!(
            selected(
                &facts,
                &change(&[
                    ("Cargo.toml", &base, head),
                    ("Cargo.lock", &locks.0, &locks.0)
                ])
            ),
            vec![RustSelection::None, RustSelection::None],
            "{head}"
        );
    }
    assert_eq!(
        selected(&facts, &change(&[("Cargo.toml", &base, &exempt[0])])),
        vec![
            all(AllTrigger::WorkspaceManifest),
            all(AllTrigger::WorkspaceManifest)
        ],
        "a manifest change with no lockfile change"
    );
    assert_eq!(
        selected(
            &facts,
            &change(&[
                ("Cargo.toml", &base, &exempt[1]),
                ("Cargo.lock", &locks.0, &locks.1),
            ])
        ),
        vec![
            some(&[("tool", lock_reason("fnv"))]),
            some(&[("cli", depends_on("tool"))])
        ]
    );
}

#[test]
fn any_other_root_manifest_change_selects_everything() {
    let facts = two_workspaces();
    let base = manifest(
        "serde = \"1\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
        "",
    );
    let global = [
        manifest(
            "serde = \"1\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\nhex = \"1\"\n",
            "",
        ),
        manifest("serde = \"1\"\n", ""),
        manifest(
            "serde = \"1\"\nfnv = { version = \"1.0.5\", features = [\"alloc\"] }\n",
            "",
        ),
        manifest(
            "serde = { version = \"1\" }\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
            "",
        ),
        manifest(
            "serde = \"1\"\nfnv = { path = \"fnv\", features = [\"std\"] }\n",
            "",
        ),
        manifest(
            "serde = \"2\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
            "resolver = \"3\"\n",
        ),
        manifest(
            "serde = \"2\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
            "[workspace.package]\nversion = \"1.0.0\"\n",
        ),
        manifest(
            "serde = \"2\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
            "[patch.crates-io]\nfnv = { path = \"fnv\" }\n",
        ),
        "not toml [".to_owned(),
    ];
    let unchanged_lock = lock(&workspace());
    for head in &global {
        assert_eq!(
            selected(
                &facts,
                &change(&[
                    ("Cargo.toml", &base, head),
                    ("Cargo.lock", &unchanged_lock, &unchanged_lock)
                ])
            ),
            vec![
                all(AllTrigger::WorkspaceManifest),
                all(AllTrigger::WorkspaceManifest)
            ],
            "{head}"
        );
    }
    let versions_only = manifest(
        "serde = \"2\"\nfnv = { version = \"1.0.5\", features = [\"std\"] }\n",
        "",
    );
    let both = |lock: &str| {
        change(&[
            ("Cargo.toml", &base, &versions_only),
            ("Cargo.lock", lock, lock),
        ])
    };
    let before = lock(&workspace());
    let mut no_base = both(&before);
    no_base.base_files.remove(&path("Cargo.toml"));
    let mut no_head = both(&before);
    no_head.head_files.remove(&path("Cargo.toml"));
    for changes in [no_base, no_head] {
        assert_eq!(
            selected(&facts, &changes),
            vec![
                all(AllTrigger::WorkspaceManifest),
                all(AllTrigger::WorkspaceManifest)
            ]
        );
    }
}

#[test]
fn a_rule_of_the_project_for_a_lockfile_or_a_root_manifest_wins_over_the_lockfile_rule() {
    let base = workspace();
    let before = lock(&base);
    let fnv = lock(&replaced(&base, krate("fnv", "1.0.6", &[])));
    let mut facts = two_workspaces();
    facts.config.non_inputs = vec![glob("Cargo.lock")];
    let lock_change = change(&[("Cargo.lock", &before, &fnv)]);
    assert_eq!(
        selected(&facts, &lock_change),
        vec![RustSelection::None, RustSelection::None]
    );
    facts.config.non_inputs.clear();
    facts.config.inputs = vec![InputRule {
        glob: glob("Cargo.lock"),
        packages: vec![name("tool")],
    }];
    let mapped = vec![
        some(&[("tool", SelectionReason::Input(path("Cargo.lock")))]),
        some(&[("cli", depends_on("tool"))]),
    ];
    assert_eq!(selected(&facts, &lock_change), mapped);
    let mut no_base = lock_change.clone();
    no_base.base_files.clear();
    assert_eq!(selected(&facts, &no_base), mapped);

    facts.config.inputs = vec![InputRule {
        glob: glob("Cargo.toml"),
        packages: vec![name("tool")],
    }];
    let old = manifest("serde = \"1\"\n", "");
    let new = manifest("serde = \"2\"\n", "");
    assert_eq!(
        selected(
            &facts,
            &change(&[("Cargo.toml", &old, &new), ("Cargo.lock", &before, &before)])
        ),
        vec![
            some(&[("tool", SelectionReason::Input(path("Cargo.toml")))]),
            some(&[("cli", depends_on("tool"))]),
        ]
    );
}

#[test]
fn a_version_bump_with_its_lockfile_stays_global_when_a_project_rule_decides_the_lockfile() {
    let before = lock(&workspace());
    let bumped = lock(&replaced(&workspace(), krate("fnv", "1.0.6", &[])));
    let old = manifest("serde = \"1\"\n", "");
    let new = manifest("serde = \"2\"\n", "");
    let changes = change(&[("Cargo.toml", &old, &new), ("Cargo.lock", &before, &bumped)]);
    let global = vec![
        all(AllTrigger::WorkspaceManifest),
        all(AllTrigger::WorkspaceManifest),
    ];
    let mut facts = two_workspaces();
    assert_eq!(
        selected(&facts, &changes),
        vec![
            some(&[("tool", lock_reason("fnv"))]),
            some(&[("cli", depends_on("tool"))])
        ]
    );
    facts.config.non_inputs = vec![glob("Cargo.lock")];
    assert_eq!(selected(&facts, &changes), global);
    facts.config.non_inputs.clear();
    facts.config.inputs = vec![InputRule {
        glob: glob("Cargo.lock"),
        packages: vec![name("core")],
    }];
    assert_eq!(selected(&facts, &changes), global);
}
