// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;

use super::change_time::{
    INPUT, INPUTS_RESTORE, OLD, OUTPUT, assert_fresh, assert_stale, at, built_repository,
    restore_inputs, saved, settled_after,
};
use super::inventory::build_stamping;
use super::{own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const ALIAS: &str = "ties/alias.txt";

fn aliased() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, INPUT, "");
    fs::hard_link(repo.root().join(INPUT), repo.root().join(ALIAS))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged(".git/info/exclude", "ties/alias.txt\n");
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../ties/alias.txt\n",
    );
    repo.set_mtime(INPUT, at(OLD));
    settled_after(&repo, INPUT);
    build_stamping(&repo, OUTPUT, None, &[]);
    repo
}

#[test]
fn a_hard_link_alias_of_a_restored_input_is_fresh() {
    let repo = aliased();
    saved(&repo);
    repo.set_mtime(INPUT, at(OLD + 1));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    assert_eq!(repo.mtime(ALIAS), at(OLD));
    assert_fresh(&repo, "ties/alias.txt\tlib\n");
}

#[test]
fn a_hard_link_alias_changed_after_the_restore_is_stale() {
    let repo = aliased();
    saved(&repo);
    repo.set_mtime(INPUT, at(OLD + 1));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    repo.write_unstaged(ALIAS, "");
    repo.set_mtime(ALIAS, at(OLD));
    assert_stale(&repo);
}

fn from_the_rust_root(repo: &Repo, operation: &str) {
    let ran = super::velnor_ci()
        .current_dir(repo.root().join("rust"))
        .env("GITHUB_JOB", "job")
        .args(["inputs", operation, "--target-dir", "target"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(ran.status.success(), "{ran:?}");
}

#[test]
fn a_restore_of_another_root_keeps_the_inputs_an_earlier_restore_vouched() {
    let (repo, _) = built_repository();
    saved(&repo);
    from_the_rust_root(&repo, "save");
    repo.set_mtime(INPUT, at(OLD + 1));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    from_the_rust_root(&repo, "restore");
    assert_fresh(&repo, "ties/stamped.txt\tlib\n");
}
