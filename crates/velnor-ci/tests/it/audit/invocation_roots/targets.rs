// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;

use super::{WORKSPACE_TARGET, placed, unit, workspace_output, workspace_with_members};

const REFUSAL: &str = "outside this command's explicit selection";

fn stderr_with_untrusted_y(repo: &Repo, args: &[&str]) -> String {
    placed(repo, &format!("{WORKSPACE_TARGET}/debug"));
    unit(
        repo,
        &format!("{WORKSPACE_TARGET}/debug/build/y-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    let mut command = vec!["cargo", "build"];
    command.extend_from_slice(args);
    command.push("--no-such-flag");
    workspace_output(repo, &command)
}

fn assert_selected(stderr: &str, context: &str) {
    assert!(
        !stderr.contains(REFUSAL) && stderr.contains("build command failed"),
        "{context}: {stderr}"
    );
}

fn assert_refused(stderr: &str, context: &str) {
    assert!(
        stderr.contains(&format!("{REFUSAL}: y (build-script output)")),
        "{context}: {stderr}"
    );
}

#[test]
fn a_package_with_no_target_of_the_named_kind_is_not_selected_by_a_plural_filter() {
    for filter in ["--bins", "--examples", "--bin=y", "--example=y"] {
        let stderr = stderr_with_untrusted_y(&workspace_with_members(), &["-p", "y", filter]);
        assert_refused(&stderr, filter);
    }
    for filter in ["--lib", "--tests", "--benches", "--all-targets"] {
        let stderr = stderr_with_untrusted_y(&workspace_with_members(), &["-p", "y", filter]);
        assert_selected(&stderr, filter);
    }
}

fn with_a_binary_named_like_the_package() -> Repo {
    let repo = workspace_with_members();
    unit(&repo, "y/src/main.rs", "fn main() {}\n");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_singular_target_filter_selects_a_package_only_when_a_target_has_that_name() {
    for (filter, selected) in [
        (&["--bin", "missing"][..], false),
        (&["--bin=missing"], false),
        (&["--bin", "y"], true),
        (&["--bin=y"], true),
        (&["--bin", "y*"], true),
        (&["--bin", "m*"], false),
        (&["--bin"], true),
        (&["--example", "y"], false),
    ] {
        let mut selection = vec!["-p", "y"];
        selection.extend_from_slice(filter);
        let stderr = stderr_with_untrusted_y(&with_a_binary_named_like_the_package(), &selection);
        if selected {
            assert_selected(&stderr, &format!("{filter:?}"));
        } else {
            assert_refused(&stderr, &format!("{filter:?}"));
        }
    }
}

fn with_only_a_feature_gated_binary() -> Repo {
    let repo = workspace_with_members();
    unit(
        &repo,
        "y/Cargo.toml",
        "[package]\nname = \"y\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nextra = []\n\n[[bin]]\nname = \"y\"\npath = \"src/main.rs\"\nrequired-features = [\"extra\"]\n",
    );
    unit(&repo, "y/src/main.rs", "fn main() {}\n");
    fs::remove_file(repo.root().join("y/src/lib.rs")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_workspace_build_does_not_select_a_member_whose_only_target_needs_disabled_features() {
    for selection in [
        &["--workspace", "--exclude", "x"][..],
        &["--workspace", "--exclude=x"],
        &["-p", "y"],
    ] {
        let stderr = stderr_with_untrusted_y(&with_only_a_feature_gated_binary(), selection);
        assert_refused(&stderr, &format!("{selection:?}"));
    }
}

#[test]
fn a_workspace_build_selects_a_member_whose_feature_gated_target_is_enabled() {
    for selection in [
        &["--workspace", "--exclude", "x", "--all-features"][..],
        &["-p", "y", "--all-features"],
    ] {
        let stderr = stderr_with_untrusted_y(&with_only_a_feature_gated_binary(), selection);
        assert_selected(&stderr, &format!("{selection:?}"));
    }
}

#[test]
fn a_plan_only_cargo_invocation_is_refused_before_anything_is_invalidated() {
    for selection in [
        &["build", "--unit-graph"][..],
        &["build", "--build-plan"],
        &["check", "--unit-graph", "--workspace", "--exclude", "x"],
        &["rustc", "-p", "y", "--print=cfg"],
        &["rustc", "-p", "y", "--print", "cfg"],
    ] {
        let repo = workspace_with_members();
        placed(&repo, &format!("{WORKSPACE_TARGET}/debug"));
        let output = format!("{WORKSPACE_TARGET}/debug/build/y-f000000000000005/output");
        unit(&repo, &output, "cargo:rerun-if-changed=src/lib.rs\n");
        let mut command = vec!["cargo"];
        command.extend_from_slice(selection);
        let stderr = workspace_output(&repo, &command);
        assert!(
            stderr.contains("plan-only Cargo invocation"),
            "{selection:?}: {stderr}"
        );
        assert!(
            repo.root().join(&output).exists(),
            "{selection:?}: the declaration was invalidated"
        );
    }
}
