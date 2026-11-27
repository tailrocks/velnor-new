// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Real cargo, real audit, real plan: every file a build reads must select a package.

mod fixture;
mod oracle;

use std::collections::BTreeSet;
use std::fs;

use fixture::{PIN, fixture};
use oracle::{Layout, Share, audit_written, build, named_reads, planned, tracked};

const PRECISE: [&str; 7] = ["kept", "gen", "pm", "puser", "root", "nested", "outenv"];

const ROWS: [(&str, &str); 18] = [
    ("docs/macro.md", "user"),
    ("crates/docs/near.md", "user"),
    ("docs/pm.md", "puser"),
    ("docs/gen.md", "gen"),
    ("docs/ovr.md", "ovr"),
    ("docs/ovr2.md", "ovr2"),
    ("docs/outenv.md", "outenv"),
    ("docs/ext.md", "euser"),
    ("docs/ext.md", "muser"),
    ("docs/ext.md", "asstd"),
    ("docs/root.md", "root"),
    ("crates/nested/src/n.md", "nested"),
    ("docs/none.md", "none"),
    ("docs/cond.md", "cond"),
    ("docs/tofile.md", "tofile"),
    ("docs/early.md", "early"),
    ("docs/shadow.md", "shadow"),
    ("docs/envdecl.md", "envdecl"),
];

fn lock_of(repo: &crate::io::Repo) -> String {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock")).unwrap_or_default()
}

fn oracle(share: Share, layout: Layout) {
    let repo = fixture();
    build(&repo, share, layout);
    audit_written(&repo);
    let tracked = tracked(&repo);
    let named: Vec<String> = named_reads(&repo)
        .into_iter()
        .filter(|path| tracked.contains(path))
        .collect();
    let silent: Vec<&String> = named
        .iter()
        .filter(|path| planned(&repo, path).is_empty())
        .collect();
    assert!(
        silent.is_empty(),
        "a change to these files selects nothing, yet a build reads them: {silent:?}\n{}",
        lock_of(&repo)
    );
    let missed: Vec<String> = ROWS
        .iter()
        .filter(|(file, reader)| !planned(&repo, file).iter().any(|(name, _)| name == reader))
        .map(|(file, reader)| format!("{file} must select {reader}"))
        .collect();
    assert!(missed.is_empty(), "{missed:?}\n{}", lock_of(&repo));
    let unrelated: BTreeSet<String> = planned(&repo, "docs/other.md")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let wrongly: Vec<&str> = PRECISE
        .iter()
        .copied()
        .filter(|name| unrelated.contains(*name))
        .collect();
    assert!(
        wrongly.is_empty(),
        "docs/other.md selects {wrongly:?}\n{}",
        lock_of(&repo)
    );
    assert!(
        planned(&repo, "docs/kept.md")
            .iter()
            .any(|(name, _)| name == "kept"),
        "{}",
        lock_of(&repo)
    );
}

#[test]
fn a_file_a_real_build_reads_selects_a_package_in_the_legacy_build_layout() {
    oracle(Share::Zero, Layout::Legacy);
}

#[test]
fn a_file_a_real_build_reads_selects_a_package_in_the_new_build_layout() {
    oracle(Share::Zero, Layout::New);
}

#[test]
fn a_build_that_lets_mbx_share_build_script_outputs_is_audited_the_same() {
    oracle(Share::Shared, Layout::New);
}

#[test]
fn the_oracle_plans_at_the_pin_it_names() {
    let repo = fixture();
    let lock = fs::read_to_string(repo.root().join(".velnor/rust-ci.lock")).unwrap_or_default();
    assert!(lock.starts_with(&format!("version = \"{PIN}\"")), "{lock}");
}
