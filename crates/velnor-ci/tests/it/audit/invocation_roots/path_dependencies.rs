// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::super::root_package_with_path_dependencies;
use super::{command, unit};

const REFUSAL: &str = "outside this command's explicit selection";

fn stderr_of_build_with_untrusted_output_in(
    repo: &Repo,
    package: &str,
    selection: &[&str],
) -> String {
    unit(
        repo,
        &format!("target/debug/build/{package}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    let output = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "cargo", "build"])
        .args(selection)
        .arg("--no-such-flag")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stderr_of_build_with_untrusted_output_of(package: &str, selection: &[&str]) -> String {
    stderr_of_build_with_untrusted_output_in(
        &root_package_with_path_dependencies(),
        package,
        selection,
    )
}

fn assert_selected(stderr: &str, context: &str) {
    assert!(
        !stderr.contains(REFUSAL) && stderr.contains("build command failed"),
        "{context}: {stderr}"
    );
}

fn assert_refused(stderr: &str, context: &str) {
    assert!(stderr.contains(REFUSAL), "{context}: {stderr}");
}

#[test]
fn a_workspace_build_selects_the_path_dependencies_outside_the_workspace() {
    for selection in [
        &["--workspace"][..],
        &["--workspace", "--all-targets"],
        &["--all"],
    ] {
        for package in ["mid", "leaf", "side"] {
            let stderr = stderr_of_build_with_untrusted_output_of(package, selection);
            assert_selected(&stderr, &format!("{selection:?} {package}"));
        }
    }
}

#[test]
fn a_workspace_build_does_not_select_the_path_dependencies_of_an_excluded_member() {
    for (selection, refused, selected) in [
        (&["--workspace", "--exclude", "other"][..], "mid", "side"),
        (&["--workspace", "--exclude=other"], "leaf", "side"),
        (&["--workspace", "--exclude", "top"], "side", "mid"),
        (&["--all", "--exclude=top"], "side", "leaf"),
    ] {
        let stderr = stderr_of_build_with_untrusted_output_of(refused, selection);
        assert_refused(&stderr, &format!("{selection:?} {refused}"));
        let stderr = stderr_of_build_with_untrusted_output_of(selected, selection);
        assert_selected(&stderr, &format!("{selection:?} {selected}"));
    }
}

#[test]
fn a_package_build_selects_the_path_dependencies_of_that_package_only() {
    for package in ["mid", "leaf"] {
        let stderr = stderr_of_build_with_untrusted_output_of(package, &["-p", "other"]);
        assert_selected(&stderr, &format!("-p other {package}"));
    }
    let stderr = stderr_of_build_with_untrusted_output_of("side", &["-p", "other"]);
    assert_refused(&stderr, "-p other side");
    let stderr = stderr_of_build_with_untrusted_output_of("mid", &["-p", "top"]);
    assert_refused(&stderr, "-p top mid");
}

#[test]
fn a_package_build_selects_the_members_it_depends_on() {
    let repo = root_package_with_path_dependencies();
    unit(
        &repo,
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[[test]]\nname = \"ext\"\npath = \"ext.rs\"\n\n[dependencies]\nside = { path = \"vendor/side\" }\nother = { path = \"other\" }\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    repo.git(&["add", "--all"]);
    for package in ["other", "mid"] {
        let stderr = stderr_of_build_with_untrusted_output_in(&repo, package, &["-p", "top"]);
        assert_selected(&stderr, &format!("-p top {package}"));
    }
}

fn with_dev_only_path_dependency() -> Repo {
    let repo = root_package_with_path_dependencies();
    unit(
        &repo,
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[[test]]\nname = \"ext\"\npath = \"../ext.rs\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n\n[dev-dependencies]\ntool = { path = \"../vendor/tool\" }\n",
    );
    unit(
        &repo,
        "vendor/tool/Cargo.toml",
        "[package]\nname = \"tool\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    );
    unit(&repo, "vendor/tool/src/lib.rs", "");
    unit(&repo, "vendor/tool/build.rs", "");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_build_without_test_targets_does_not_select_a_dev_only_path_dependency() {
    for selection in [&["--workspace"][..], &["-p", "other"]] {
        let stderr = stderr_of_build_with_untrusted_output_in(
            &with_dev_only_path_dependency(),
            "tool",
            selection,
        );
        assert_refused(&stderr, &format!("{selection:?}"));
    }
    for selection in [
        &["--workspace", "--all-targets"][..],
        &["-p", "other", "--tests"],
    ] {
        let stderr = stderr_of_build_with_untrusted_output_in(
            &with_dev_only_path_dependency(),
            "tool",
            selection,
        );
        assert_selected(&stderr, &format!("{selection:?}"));
    }
}
