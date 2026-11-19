// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    ChangeSet, DependencyKind, RepoFacts, RepoPath, RepoView, RustSelection, SelectionReason,
    TargetKind, Unread,
};
use velnor_rust::{detect, unread_packages};

use crate::facts::{TestResult, lib, package, repo};
use crate::select::{assert_selects, changed, depends_on, name, path, some, unknown};

const LOOP: &str = "a/excluded/loop/Cargo.toml";
const LEAF: &str = "vendor/leaf/Cargo.toml";

const MANIFESTS: [(&str, &str); 6] = [
    (
        "a/Cargo.toml",
        "[workspace]\n\n[patch.crates-io]\npatched = { path = \"../vendor/patched\" }\n",
    ),
    (
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n\n[target.'cfg(unix)'.dependencies]\nx = { path = \"../../a/x\" }\n\n[build-dependencies]\nrenamed = { version = \"1\", package = \"w\" }\n",
    ),
    (
        LEAF,
        "[package]\nname = \"leaf\"\n\n[dependencies]\ny = { path = \"../../b/y\" }\nmid = { path = \"../mid\" }\npatched = \"1\"\n\n[target.'cfg(windows)'.build_dependencies]\nv = \"1\"\n\n[dev-dependencies]\np = { path = \"../../b/p\" }\n",
    ),
    (
        "vendor/patched/Cargo.toml",
        "[package]\nname = \"patched\"\n\n[dependencies]\nq = { path = \"../../b/q\" }\n",
    ),
    (
        "a/excluded/Cargo.toml",
        "[workspace]\nmembers = [\"loop\"]\n\n[workspace.dependencies]\nz = { path = \"../../b/z\" }\nu = \"1\"\n",
    ),
    (
        LOOP,
        "[package]\nname = \"loop\"\n\n[dependencies]\nz = { workspace = true }\nu.workspace = true\n",
    ),
];

fn facts(replaced: &str, text: Option<&str>) -> Result<RepoFacts, Box<dyn std::error::Error>> {
    let mut a = vec![package(
        "app",
        "a/app/Cargo.toml",
        &[("mid", DependencyKind::Normal, "vendor/mid/Cargo.toml")],
        &[("app", TargetKind::Lib, true)],
    )?];
    for member in ["x", "a1", "a2", "a3"] {
        a.push(lib(member, &format!("a/{member}/Cargo.toml"))?);
    }
    let mut b = vec![package(
        "base",
        "b/base/Cargo.toml",
        &[("loop", DependencyKind::Normal, LOOP)],
        &[("base", TargetKind::Lib, true)],
    )?];
    for member in ["p", "q", "u", "v", "w", "y", "z", "f"] {
        b.push(lib(member, &format!("b/{member}/Cargo.toml"))?);
    }
    let files: Vec<(&str, &str)> = MANIFESTS
        .iter()
        .filter_map(|(file, original)| {
            if *file == replaced {
                Some((*file, text?))
            } else {
                Some((*file, *original))
            }
        })
        .collect();
    let view: RepoView = repo(vec![("a/Cargo.toml", a), ("b/Cargo.toml", b)], &files)?;
    Ok(detect(&view)?)
}

fn unread_after(facts: &RepoFacts, files: &[&str]) -> BTreeMap<RepoPath, Unread> {
    let changes = ChangeSet {
        paths: files.iter().map(|file| path(file)).collect::<BTreeSet<_>>(),
        base_files: BTreeMap::new(),
        head_files: BTreeMap::new(),
    };
    unread_packages(facts, &changes)
}

static THROUGH_MID: [(&str, &str); 4] = [
    ("y", "b/y/src/lib.rs"),
    ("w", "b/w/src/lib.rs"),
    ("v", "b/v/src/lib.rs"),
    ("q", "b/q/src/lib.rs"),
];
static THROUGH_LOOP: [(&str, &str); 2] = [("z", "b/z/src/lib.rs"), ("u", "b/u/src/lib.rs")];
static INSIDE: [&str; 4] = [
    "vendor/leaf/src/lib.rs",
    "a/excluded/loop/src/lib.rs",
    "vendor/mid/Cargo.toml",
    "a/excluded/Cargo.toml",
];

#[test]
fn a_change_selects_only_the_members_that_reach_it_through_the_edges_of_unowned_packages()
-> TestResult {
    let facts = facts("", None)?;
    let none = RustSelection::None;
    let app = ("app", depends_on("mid"));
    let mut rows: Vec<(&[&str], Vec<RustSelection>)> = vec![
        (
            &["b/p/src/lib.rs"],
            vec![none.clone(), some(&[("p", changed("b/p/src/lib.rs"))])],
        ),
        (
            &["a/x/src/lib.rs"],
            vec![
                some(&[("x", changed("a/x/src/lib.rs")), app.clone()]),
                none.clone(),
            ],
        ),
    ];
    rows.extend(THROUGH_MID.iter().map(|(member, file)| {
        (
            std::slice::from_ref(file),
            vec![
                some(std::slice::from_ref(&app)),
                some(&[(member, changed(file))]),
            ],
        )
    }));
    rows.extend(THROUGH_LOOP.iter().map(|(member, file)| {
        (
            std::slice::from_ref(file),
            vec![
                none.clone(),
                some(&[(member, changed(file)), ("base", depends_on("loop"))]),
            ],
        )
    }));
    rows.extend(INSIDE.iter().map(|file| {
        (
            std::slice::from_ref(file),
            vec![unknown(file), unknown(file)],
        )
    }));
    assert_selects(&facts, rows);
    assert!(unread_after(&facts, &["b/y/src/lib.rs"]).is_empty());
    Ok(())
}

#[test]
fn an_unowned_package_whose_edges_cannot_be_read_is_reached_by_every_change() -> TestResult {
    let manifest =
        |dependency: &str| format!("[package]\nname = \"loop\"\n\n[dependencies]\n{dependency}\n");
    let cases = [
        (
            Some("[package\nname = \"loop\"\n".to_owned()),
            Unread::Invalid(String::new()),
        ),
        (
            Some(manifest("missing = { workspace = true }")),
            Unread::Inherited("missing".to_owned()),
        ),
        (
            Some(manifest(
                "remote = { git = \"https://example.com/remote\" }",
            )),
            Unread::Git("remote".to_owned()),
        ),
        (
            Some(manifest("far = { path = \"../../../../far\" }")),
            Unread::Outside("far".to_owned()),
        ),
        (None, Unread::Missing),
    ];
    let unread = ("base", SelectionReason::DependsOnUnread(name("loop")));
    for (text, cause) in cases {
        let facts = facts(LOOP, text.as_deref())?;
        assert_selects(
            &facts,
            vec![
                (
                    &["b/p/src/lib.rs"],
                    vec![
                        RustSelection::None,
                        some(&[("p", changed("b/p/src/lib.rs")), unread.clone()]),
                    ],
                ),
                (
                    &["b/y/src/lib.rs"],
                    vec![
                        some(&[("app", depends_on("mid"))]),
                        some(&[("y", changed("b/y/src/lib.rs")), unread.clone()]),
                    ],
                ),
                (&[], vec![RustSelection::None, RustSelection::None]),
            ],
        );
        let found = unread_after(&facts, &["b/p/src/lib.rs"]);
        let reported: Vec<(&str, &Unread)> = found.iter().map(|(m, c)| (m.as_str(), c)).collect();
        match (&cause, reported.as_slice()) {
            (Unread::Invalid(_), [(LOOP, Unread::Invalid(detail))]) if !detail.is_empty() => {}
            (expected, [(LOOP, found)]) => assert_eq!(found, &expected),
            (_, other) => return Err(format!("{cause:?}: {other:?}").into()),
        }
        assert!(unread_after(&facts, &[]).is_empty());
    }
    let facts = facts(LEAF, None)?;
    assert_selects(
        &facts,
        vec![(
            &["b/p/src/lib.rs"],
            vec![
                some(&[("app", depends_on("mid"))]),
                some(&[("p", changed("b/p/src/lib.rs"))]),
            ],
        )],
    );
    assert_eq!(
        unread_after(&facts, &["b/p/src/lib.rs"]),
        BTreeMap::from([(path(LEAF), Unread::Missing)])
    );
    Ok(())
}
