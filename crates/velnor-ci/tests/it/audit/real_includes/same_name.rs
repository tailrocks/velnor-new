// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::unit;
use super::lock_of;
use super::oracle::{Layout, Share, build_in};
use super::two_roots::{DECLARES, audit_after, remove_named, repo, with_script, workspace};
use crate::io::Repo;

const A_LINE: &str = ".\tx\ta/x/Cargo.toml";
const B_LINE: &str = ".\tx\tb/x/Cargo.toml";

fn audited(repo: &Repo, roots: &[&str], deleted_in: &[&str], layout: Layout) -> String {
    repo.git(&["add", "--all"]);
    for root in roots {
        build_in(repo, root, Share::Zero, layout);
    }
    for root in deleted_in {
        assert!(remove_named(&repo.root().join(root).join("target"), "root-output") > 0);
    }
    let written = audit_after(repo);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    lock_of(repo)
}

fn only_b(lock: &str) {
    assert!(lock.lines().any(|line| line == B_LINE), "{lock}");
    assert!(!lock.lines().any(|line| line == A_LINE), "{lock}");
}

fn declared_beside_undeclared(layout: Layout) {
    let repo = repo("audit-three-roots");
    with_script(&repo, "a", "x", "version = \"0.1.0\"\n", DECLARES);
    with_script(&repo, "b", "x", "version = \"0.2.0\"\n", "");
    consumer(&repo, "c");
    only_b(&audited(&repo, &["a", "b", "c"], &["c"], layout));
}

fn plain_beside_undeclared(layout: Layout) {
    let repo = repo("audit-plain-beside");
    workspace(&repo, "a", "x", "version = \"0.1.0\"\n");
    unit(&repo, "a/x/src/lib.rs", "pub const A: u8 = 1;\n");
    with_script(&repo, "b", "x", "version = \"0.2.0\"\n", "");
    consumer(&repo, "c");
    only_b(&audited(&repo, &["a", "b", "c"], &["b", "c"], layout));
}

fn consumer(repo: &Repo, root: &str) {
    workspace(
        repo,
        root,
        "y",
        "\n[dependencies]\nx = { path = \"../../a/x\" }\nx2 = { package = \"x\", path = \"../../b/x\" }\n",
    );
    unit(
        repo,
        &format!("{root}/y/src/lib.rs"),
        "pub use x2::O as O2;\npub use x::*;\n",
    );
}

fn member_beside_renamed(layout: Layout) {
    let repo = repo("audit-member-beside");
    unit(
        &repo,
        "a/Cargo.toml",
        "[workspace]\nmembers = [\"x\", \"y\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "a/Cargo.lock", "version = 4\n");
    unit(
        &repo,
        "a/x/Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    );
    unit(
        &repo,
        "a/x/build.rs",
        "fn main() {\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/o.txt\"), \"o\").unwrap();\n    println!(\"cargo:rerun-if-changed=build.rs\");\n}\n",
    );
    unit(
        &repo,
        "a/x/src/lib.rs",
        "pub const O: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/o.txt\"));\n",
    );
    unit(
        &repo,
        "a/y/Cargo.toml",
        "[package]\nname = \"y\"\nedition = \"2024\"\n\n[dependencies]\nx2 = { package = \"x\", path = \"../../b/x\" }\n",
    );
    unit(&repo, "a/y/src/lib.rs", "pub use x2::O;\n");
    with_script(&repo, "b", "x", "version = \"0.2.0\"\n", "");
    only_b(&audited(&repo, &["a", "b"], &["a"], layout));
}

#[test]
fn an_undeclared_build_script_credits_nothing_to_a_same_named_package_built_in_the_same_target_in_the_legacy_layout()
 {
    declared_beside_undeclared(Layout::Legacy);
}

#[test]
fn an_undeclared_build_script_credits_nothing_to_a_same_named_package_built_in_the_same_target_in_the_new_layout()
 {
    declared_beside_undeclared(Layout::New);
}

#[test]
fn an_undeclared_build_script_credits_nothing_to_a_same_named_package_without_a_build_script_in_the_legacy_layout()
 {
    plain_beside_undeclared(Layout::Legacy);
}

#[test]
fn an_undeclared_build_script_credits_nothing_to_a_same_named_package_without_a_build_script_in_the_new_layout()
 {
    plain_beside_undeclared(Layout::New);
}

#[test]
fn a_build_scripts_own_dep_info_gets_no_run_of_a_same_named_member_in_the_legacy_layout() {
    member_beside_renamed(Layout::Legacy);
}

#[test]
fn a_build_scripts_own_dep_info_gets_no_run_of_a_same_named_member_in_the_new_layout() {
    member_beside_renamed(Layout::New);
}
