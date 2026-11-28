// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::{audit, unit};
use super::fixture::pinned_lock;
use super::lock_of;
use super::oracle::{Layout, Share, build_in};
use crate::io::Repo;

pub(super) const READS_OUT_DIR: &str =
    "pub const O: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/o.txt\"));\n";
const DECLARES_UNDER_FEATURE: &str = "    if std::env::var_os(\"CARGO_FEATURE_F\").is_some() {\n        println!(\"cargo:rerun-if-changed=build.rs\");\n    }\n";
pub(super) const DECLARES: &str = "    println!(\"cargo:rerun-if-changed=build.rs\");\n";

fn script(body: &str) -> String {
    format!(
        "fn main() {{\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{{out}}/o.txt\"), \"o\").unwrap();\n{body}}}\n"
    )
}

pub(super) fn workspace(repo: &Repo, root: &str, member: &str, manifest: &str) {
    unit(
        repo,
        &format!("{root}/Cargo.toml"),
        &format!("[workspace]\nmembers = [\"{member}\"]\nresolver = \"3\"\n"),
    );
    unit(repo, &format!("{root}/Cargo.lock"), "version = 4\n");
    unit(
        repo,
        &format!("{root}/{member}/Cargo.toml"),
        &format!("[package]\nname = \"{member}\"\nedition = \"2024\"\n{manifest}"),
    );
}

pub(super) fn with_script(repo: &Repo, root: &str, member: &str, manifest: &str, body: &str) {
    workspace(
        repo,
        root,
        member,
        &format!("build = \"build.rs\"\n{manifest}"),
    );
    unit(repo, &format!("{root}/{member}/build.rs"), &script(body));
    unit(repo, &format!("{root}/{member}/src/lib.rs"), READS_OUT_DIR);
}

pub(super) fn repo(name: &str) -> Repo {
    let repo = Repo::new(name);
    unit(&repo, ".gitignore", "target/\n");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    pinned_lock(&repo);
    repo
}

fn built_and_audited(repo: &Repo, layout: Layout) -> String {
    repo.git(&["add", "--all"]);
    for root in ["a", "b"] {
        build_in(repo, root, Share::Zero, layout);
    }
    let written = audit_after(repo);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    lock_of(repo)
}

pub(super) fn audit_after(repo: &Repo) -> std::process::Output {
    audit(repo, &["--write"])
}

pub(super) fn remove_named(directory: &Path, name: &str) -> usize {
    let mut removed = 0;
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            removed += remove_named(&path, name);
        } else if path.file_name().is_some_and(|file| file == name) {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
            removed += 1;
        }
    }
    removed
}

fn foreign_member(layout: Layout) {
    let repo = repo("audit-foreign-member");
    with_script(
        &repo,
        "a",
        "x",
        "\n[features]\ndefault = [\"f\"]\nf = []\n",
        DECLARES_UNDER_FEATURE,
    );
    workspace(
        &repo,
        "b",
        "y",
        "\n[dependencies]\nx = { path = \"../../a/x\", default-features = false }\n",
    );
    unit(&repo, "b/y/src/lib.rs", "pub use x::O;\n");
    let lock = built_and_audited(&repo, layout);
    assert!(
        lock.lines().any(|line| line == ".\tx\ta/x/Cargo.toml"),
        "{lock}"
    );
}

fn same_package_name(layout: Layout) {
    let repo = repo("audit-same-package");
    with_script(&repo, "a", "x", "", DECLARES);
    with_script(&repo, "b", "x", "", "");
    repo.git(&["add", "--all"]);
    for root in ["a", "b"] {
        build_in(&repo, root, Share::Zero, layout);
    }
    let removed = remove_named(&repo.root().join("b/target"), "root-output");
    assert!(removed > 0);
    let written = audit_after(&repo);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    let lock = lock_of(&repo);
    assert!(
        lock.lines().any(|line| line == ".\tx\tb/x/Cargo.toml"),
        "{lock}"
    );
    assert!(
        !lock.lines().any(|line| line == ".\tx\ta/x/Cargo.toml"),
        "{lock}"
    );
}

#[test]
fn a_package_of_another_root_built_without_its_default_feature_credits_its_dot_line_in_the_legacy_layout()
 {
    foreign_member(Layout::Legacy);
}

#[test]
fn a_package_of_another_root_built_without_its_default_feature_credits_its_dot_line_in_the_new_layout()
 {
    foreign_member(Layout::New);
}

#[test]
fn an_undeclared_build_script_of_one_root_credits_nothing_to_the_same_package_name_of_another_root_in_the_legacy_layout()
 {
    same_package_name(Layout::Legacy);
}

#[test]
fn an_undeclared_build_script_of_one_root_credits_nothing_to_the_same_package_name_of_another_root_in_the_new_layout()
 {
    same_package_name(Layout::New);
}
