// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{AllTrigger, RustMember, RustSelection};

use crate::cargo_lock::{change, lock, manifest, selected, two_workspaces};
use crate::lock_affected::{
    Pkg, affected, affected_with, krate, member as member_pkg, render, replaced, workspace,
};
use crate::select::{all, changed, depends_on, facts, member, path, some};

#[test]
fn a_lock_only_change_of_a_members_version_is_an_error_so_that_everything_runs() {
    let base = workspace();
    let before = render(4, &base, "");
    let bumped = |member_name: &'static str| {
        let dependencies: &[&str] = match member_name {
            "app" => &["lib"],
            "core" => &["serde"],
            "lib" => &["core", "hex"],
            _ => &["fnv", "hex"],
        };
        render(
            4,
            &replaced(
                &base,
                member_pkg(member_name, dependencies).version("0.1.0"),
            ),
            "",
        )
    };
    let rows: [(&str, &[&str], bool); 5] = [
        ("lib", &[], false),
        ("lib", &["app"], false),
        ("lib", &["lib"], true),
        ("lib", &["lib", "app"], true),
        ("tool", &["lib"], false),
    ];
    for (changed, edited, vouched) in rows {
        assert_eq!(
            affected_with(&before, &bumped(changed), edited).is_some(),
            vouched,
            "{changed} with {edited:?}"
        );
    }
}

#[test]
fn a_changed_lock_entry_that_no_member_reaches_is_an_error_so_that_everything_runs() {
    let base = workspace();
    let with_orphan = |orphan: Pkg| {
        let mut packages = base.clone();
        packages.push(orphan);
        render(4, &packages, "")
    };
    let orphan = |version: &str, dependencies: &[&str]| krate("orphan", version, dependencies);
    let before = render(4, &base, "");
    let kept = with_orphan(orphan("1.0.0", &["fnv"]));
    let rows: [(&str, String, String, bool); 7] = [
        (
            "added",
            before.clone(),
            with_orphan(orphan("1.0.0", &[])),
            false,
        ),
        (
            "added with a dependency that members reach",
            before.clone(),
            with_orphan(orphan("1.0.0", &["fnv"])),
            false,
        ),
        (
            "version changed",
            kept.clone(),
            with_orphan(orphan("1.0.1", &["fnv"])),
            false,
        ),
        (
            "dependencies changed",
            kept.clone(),
            with_orphan(orphan("1.0.0", &["hex"])),
            false,
        ),
        ("unchanged", kept.clone(), kept.clone(), true),
        ("removed", kept.clone(), before.clone(), true),
        (
            "rewired by a bump that members reach",
            kept.clone(),
            {
                let mut packages = replaced(&base, krate("fnv", "1.0.6", &[]));
                packages.push(orphan("1.0.0", &["fnv"]));
                render(4, &packages, "")
            },
            false,
        ),
    ];
    for (label, base, head, vouched) in rows {
        assert_eq!(affected(&base, &head).is_some(), vouched, "{label}");
    }
}

#[test]
fn a_lockfile_entry_that_no_member_reaches_selects_everything() {
    let facts = two_workspaces();
    let base = workspace();
    let before = lock(&base);
    let mut orphan = base.clone();
    orphan.push(krate("orphan", "1.0.0", &[]));
    assert_eq!(
        selected(&facts, &change(&[("Cargo.lock", &before, &lock(&orphan))])),
        vec![
            all(AllTrigger::WorkspaceManifest),
            all(AllTrigger::WorkspaceManifest),
        ],
        "an orphan package"
    );
}

#[test]
fn a_members_version_that_only_the_lockfile_changes_selects_everything() {
    let facts = two_workspaces();
    let before = lock(&workspace());
    let everything = vec![
        all(AllTrigger::WorkspaceManifest),
        all(AllTrigger::WorkspaceManifest),
    ];
    let lib_bumped = lock(&replaced(
        &workspace(),
        member_pkg("lib", &["core", "hex"]).version("0.1.0"),
    ));
    assert_eq!(
        selected(&facts, &change(&[("Cargo.lock", &before, &lib_bumped)])),
        everything,
        "a member version in the lockfile alone"
    );
    assert_eq!(
        selected(
            &facts,
            &change(&[
                ("Cargo.lock", &before, &lib_bumped),
                ("lib/Cargo.toml", "", "")
            ])
        ),
        vec![
            some(&[
                ("lib", changed("lib/Cargo.toml")),
                ("app", depends_on("lib"))
            ]),
            RustSelection::None
        ],
        "a member version in the lockfile and its manifest"
    );
}

#[test]
fn a_changed_version_of_a_dependency_with_a_path_or_git_source_selects_everything() {
    let facts = two_workspaces();
    let before = lock(&workspace());
    let everything = vec![
        all(AllTrigger::WorkspaceManifest),
        all(AllTrigger::WorkspaceManifest),
    ];
    let rows = [
        (
            "fnv = { path = \"fnv\", version = \"1.0.5\" }\n",
            "fnv = { path = \"fnv\", version = \"2\" }\n",
            everything.clone(),
        ),
        (
            "fnv = { version = \"1.0.5\", path = \"fnv\", features = [\"std\"] }\n",
            "fnv = { version = \"2\", path = \"fnv\", features = [\"std\"] }\n",
            everything.clone(),
        ),
        (
            "fnv = { git = \"https://example.com/fnv\", version = \"1.0.5\" }\n",
            "fnv = { git = \"https://example.com/fnv\", version = \"2\" }\n",
            everything,
        ),
        (
            "fnv = { path = \"fnv\", version = \"1.0.5\" }\nserde = \"1\"\n",
            "fnv = { path = \"fnv\", version = \"1.0.5\" }\nserde = \"2\"\n",
            vec![RustSelection::None, RustSelection::None],
        ),
    ];
    for (old, new, expected) in rows {
        assert_eq!(
            selected(
                &facts,
                &change(&[
                    ("Cargo.toml", &manifest(old, ""), &manifest(new, "")),
                    ("Cargo.lock", &before, &before),
                ])
            ),
            expected,
            "{new}"
        );
    }
}

#[test]
fn a_root_packages_version_that_only_the_lockfile_changes_selects_everything_beside_a_version_only_manifest_edit()
 {
    let facts = facts(vec![(
        "Cargo.toml",
        vec![
            RustMember {
                manifest: path("Cargo.toml"),
                directory: None,
                ..member("app", "app", &[("lib", "lib")])
            },
            member("core", "core", &[]),
            member("lib", "lib", &[("core", "core")]),
            member("tool", "tool", &[]),
        ],
    )]);
    let before = lock(&workspace());
    let root_bumped = lock(&replaced(
        &workspace(),
        member_pkg("app", &["lib"]).version("0.1.0"),
    ));
    let lib_bumped = lock(&replaced(
        &workspace(),
        member_pkg("lib", &["core", "hex"]).version("0.1.0"),
    ));
    let (old, new) = (
        manifest("serde = \"1\"\n", ""),
        manifest("serde = \"2\"\n", ""),
    );
    let edit = |head_lock: &str, members: &[&str]| {
        let mut files = vec![
            ("Cargo.toml", old.as_str(), new.as_str()),
            ("Cargo.lock", before.as_str(), head_lock),
        ];
        files.extend(members.iter().map(|path| (*path, "", "")));
        selected(&facts, &change(&files))
    };
    assert_eq!(
        edit(&root_bumped, &[]),
        vec![all(AllTrigger::WorkspaceManifest)],
        "the root package alone"
    );
    assert_eq!(
        edit(&lib_bumped, &["lib/Cargo.toml"]),
        vec![some(&[
            ("lib", changed("lib/Cargo.toml")),
            ("app", depends_on("lib"))
        ])],
        "a member with its manifest, beside the same manifest edit"
    );
}
