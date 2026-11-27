// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::process::Command;

use velnor_model::RepoPath;

use super::{MISE, NESTED, PINNED, decision, merge, world_over};
use crate::io::Repo;

type Edit = fn(&Repo);

const NESTED_OTHER: &str = "tools/gen/crates/other/src/lib.rs";

fn corpus() -> Vec<(&'static str, bool, Edit)> {
    vec![
        ("goals", true, |repo| {
            repo.write("docs/goals.md", "goals\nmore\n");
        }),
        ("readme", true, |repo| {
            repo.write("README.md", "readme\nmore\n");
        }),
        ("plain-rs", true, |repo| {
            repo.write(NESTED_OTHER, "pub fn o() -> u32 { 5 }\n");
        }),
        ("engine", false, |repo| {
            repo.write(
                "tools/gen/crates/engine/src/lib.rs",
                "pub fn note() -> &'static str { \"x\" }\n",
            );
        }),
        ("embedded", false, |repo| {
            repo.write("docs/note.md", "note\nmore\n");
        }),
        ("borrowed", false, |repo| {
            repo.write("shared/borrowed.rs", "pub fn n() -> u32 { 9 }\n");
        }),
        ("config", false, |repo| {
            repo.write(
                ".velnor/rust-ci.toml",
                "full_run = \"off\"\n\n[ci]\nskip_push_after_merge_group = true\n\n[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n",
            );
        }),
        ("include", false, |repo| {
            repo.write(
                NESTED_OTHER,
                "pub fn o() -> &'static str { include_str!(\"../../../../../docs/goals.md\") }\n",
            );
        }),
        ("folder", false, |repo| {
            repo.write(".github/notes.md", "notes\n");
        }),
    ]
}

fn sparse_clone(source: &Repo, name: &str) -> Repo {
    let clone = Repo::empty(name);
    let status = Command::new("git")
        .args(["clone", "--quiet", "--no-checkout"])
        .arg(source.root())
        .arg(clone.root())
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success(), "clone");
    clone.git(&[
        "sparse-checkout",
        "set",
        "--no-cone",
        "/tools/gen/",
        "/.velnor/",
        "/mise.toml",
        "/mise.lock",
    ]);
    clone.git(&[
        "checkout",
        "--quiet",
        "--detach",
        &super::revision(source, "HEAD"),
    ]);
    clone
}

fn rendered(source: &Repo, revision: &str, name: &str) -> BTreeMap<RepoPath, String> {
    let tree = Repo::empty(name);
    source.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        tree.root().to_str().unwrap_or_default(),
        revision,
    ]);
    let checkout =
        velnor_gen::io::read_checkout(tree.root()).unwrap_or_else(|e| unreachable!("{e}"));
    velnor_gen::generated_files(&checkout.view).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn every_skip_leaves_the_rendered_workflows_unchanged() {
    let mut skipped = Vec::new();
    for (name, skips, edit) in corpus() {
        let source = world_over(&format!("gc-render-{name}"), NESTED, PINNED, |repo| {
            repo.write_unstaged("mise.toml", MISE);
        });
        merge(&source, edit);
        let clone = sparse_clone(&source, &format!("gc-render-sparse-{name}"));
        let decided = decision(&clone);
        assert_eq!(decided == "skip", skips, "{name}: {decided}");
        let before = rendered(&source, "HEAD^1", &format!("gc-render-base-{name}"));
        let after = rendered(&source, "HEAD", &format!("gc-render-head-{name}"));
        if decided == "skip" {
            assert_eq!(before, after, "{name}");
            skipped.push(name);
        }
        if name == "config" {
            assert_ne!(before, after, "the control edit must change the rendering");
        }
    }
    assert_eq!(skipped, ["goals", "readme", "plain-rs"]);
}
