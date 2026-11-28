// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::unit;
use super::oracle::{Layout, Share, build_in};
use super::two_roots::{audit_after, repo, workspace};

fn cdylib_with_a_bin(layout: Layout) {
    let repo = repo("audit-cdylib-bin");
    workspace(
        &repo,
        "d",
        "m",
        "\n[lib]\ncrate-type = [\"cdylib\"]\n\n[[bin]]\nname = \"m\"\npath = \"src/main.rs\"\n",
    );
    unit(&repo, "d/m/src/lib.rs", "pub fn f() {}\n");
    unit(&repo, "d/m/src/main.rs", "fn main() {}\n");
    repo.git(&["add", "--all"]);
    build_in(&repo, "d", Share::Zero, layout);
    let unhashed = repo.root().join("d/target/debug/deps/m.d").is_file();
    assert_eq!(unhashed, matches!(layout, Layout::Legacy));
    let written = audit_after(&repo);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
}

#[test]
fn a_cdylib_crate_with_a_bin_built_by_real_cargo_passes_the_audit_in_the_legacy_layout() {
    cdylib_with_a_bin(Layout::Legacy);
}

#[test]
fn a_cdylib_crate_with_a_bin_built_by_real_cargo_passes_the_audit_in_the_new_layout() {
    cdylib_with_a_bin(Layout::New);
}
