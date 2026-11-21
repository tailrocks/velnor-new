// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{command, placed, unit};

mod descendant;
mod package_ids;

const GROUP_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

const SHARED_TARGET: &str = "target-velnor/tools";

fn grouped_tools() -> Repo {
    grouped_packages([("a", "a"), ("b", "b")])
}

fn untrusted_b_output(repo: &Repo) {
    untrusted_output(repo, "b");
}

fn inputs_build_in(repo: &Repo, root: &str, cargo: &[&str]) -> std::process::Output {
    command(env!("CARGO_BIN_EXE_velnor-ci"))
        .env("CARGO_TARGET_DIR", repo.root().join(SHARED_TARGET))
        .current_dir(repo.root().join(root))
        .args(["inputs", "build", "--"])
        .args(cargo)
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn failed_build_in_tools_a_with_untrusted_b_output(failing: &[&str]) -> Repo {
    let repo = grouped_tools();
    let failed = inputs_build_in(&repo, "tools/a", failing);
    assert!(!failed.status.success(), "{failed:?}");
    assert!(
        repo.root()
            .join(SHARED_TARGET)
            .join(".velnor-input-inventories/failed-inputs-build.json")
            .exists(),
        "the failed build was not recorded in the shared target directory"
    );
    untrusted_b_output(&repo);
    repo
}

fn stderr_of_tools_b_build(repo: &Repo, args: &[&str]) -> String {
    let output = inputs_build_in(repo, "tools/b", args);
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn a_failed_build_in_tools_a_does_not_blame_tools_b_after_it(failing: &[&str]) {
    let repo = failed_build_in_tools_a_with_untrusted_b_output(failing);
    let refused = inputs_build_in(&repo, "tools/b", &["cargo", "build"]);
    let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert_eq!(refused.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("explicit selection: b (build-script output). This usually means"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("An earlier inputs build failed"),
        "{stderr}"
    );
}

#[test]
fn a_failed_build_in_one_root_does_not_blame_another_roots_package_in_the_shared_target() {
    a_failed_build_in_tools_a_does_not_blame_tools_b_after_it(&[
        "cargo",
        "build",
        "-p",
        "a",
        "-p",
        "b",
        "--no-such-flag",
    ]);
}

#[test]
fn a_failed_workspace_build_in_one_root_does_not_blame_another_roots_package_in_the_shared_target()
{
    a_failed_build_in_tools_a_does_not_blame_tools_b_after_it(&[
        "cargo",
        "build",
        "--workspace",
        "--no-such-flag",
    ]);
}

const MEMBER_PACKAGE_SPELLINGS: [&[&str]; 7] = [
    &["cargo", "build", "-p", "b", "--no-such-flag"],
    &["cargo", "build", "--package", "b", "--no-such-flag"],
    &["cargo", "build", "--package=b", "--no-such-flag"],
    &["cargo", "build", "-pb", "--no-such-flag"],
    &["cargo", "build", "-p=b", "--no-such-flag"],
    &["cargo", "build", "-p", "b@0.1.0", "--no-such-flag"],
    &["cargo", "build", "--package=b@0.1.0", "--no-such-flag"],
];

const NON_MEMBER_PACKAGE_SPELLINGS: [&[&str]; 6] = [
    &["cargo", "build", "-p", "no-such", "--no-such-flag"],
    &["cargo", "build", "--package=no-such", "--no-such-flag"],
    &["cargo", "build", "-pno-such", "--no-such-flag"],
    &[
        "cargo",
        "build",
        "--package=git+https://example.invalid/app#app@0.1.0",
        "--no-such-flag",
    ],
    &[
        "cargo",
        "build",
        "-p",
        "registry+https://github.com/rust-lang/crates.io-index#b@0.1.0",
        "--no-such-flag",
    ],
    &[
        "cargo",
        "build",
        "--package=git+https://example.invalid/b#b:0.1.0",
        "--no-such-flag",
    ],
];

#[test]
fn every_spelling_of_a_member_package_flag_selects_that_member_in_another_roots_build() {
    for args in MEMBER_PACKAGE_SPELLINGS {
        let repo = failed_build_in_tools_a_with_untrusted_b_output(&[
            "cargo",
            "build",
            "-p",
            "a",
            "-p",
            "b",
            "--no-such-flag",
        ]);
        let stderr = stderr_of_tools_b_build(&repo, args);
        assert!(
            !stderr.contains("outside this command's explicit selection"),
            "{args:?}: {stderr}"
        );
        assert!(
            stderr.contains("build command failed"),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn every_spelling_of_a_non_member_package_flag_refuses_the_untrusted_member_output() {
    for args in NON_MEMBER_PACKAGE_SPELLINGS {
        let repo = failed_build_in_tools_a_with_untrusted_b_output(&[
            "cargo",
            "build",
            "-p",
            "a",
            "-p",
            "b",
            "--no-such-flag",
        ]);
        let stderr = stderr_of_tools_b_build(&repo, args);
        assert!(
            stderr.contains("outside this command's explicit selection: b (build-script output)"),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn a_failed_build_records_only_its_own_roots_selection() {
    for failing in [
        &["cargo", "build", "--workspace", "--no-such-flag"][..],
        &["cargo", "build", "-p", "a", "-p", "b", "--no-such-flag"],
    ] {
        let repo = grouped_tools();
        let failed = inputs_build_in(&repo, "tools/a", failing);
        assert!(!failed.status.success(), "{failing:?}: {failed:?}");
        let marker = std::fs::read(
            repo.root()
                .join(SHARED_TARGET)
                .join(".velnor-input-inventories/failed-inputs-build.json"),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        let recorded: serde_json::Value =
            serde_json::from_slice(&marker).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            recorded["selection"]["packages"],
            serde_json::json!(["a"]),
            "{failing:?}"
        );
    }
}

fn grouped_packages(packages: [(&str, &str); 2]) -> Repo {
    let repo = Repo::new("audit-invocation-roots");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        ".velnor/rust-ci.toml",
        "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n",
    );
    unit(
        &repo,
        ".velnor/rust-ci.lock",
        &GROUP_LOCK.replace("version = \"0.1.81\"", "version = \"0.1.96\""),
    );
    for (directory, package) in packages {
        unit(
            &repo,
            &format!("tools/{directory}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(
            &repo,
            &format!("tools/{directory}/Cargo.lock"),
            &format!("version = 4\n\n[[package]]\nname = \"{package}\"\nversion = \"0.1.0\"\n"),
        );
        unit(&repo, &format!("tools/{directory}/src/lib.rs"), "");
    }
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_failed_build_in_one_root_does_not_blame_a_same_named_package_in_another_root() {
    let repo = grouped_packages([("a", "util"), ("b", "util")]);
    let failed = inputs_build_in(
        &repo,
        "tools/a",
        &["cargo", "build", "-p", "util", "--no-such-flag"],
    );
    assert!(!failed.status.success(), "{failed:?}");
    untrusted_output(&repo, "util");
    let stderr = stderr_of_tools_b_build(&repo, &["cargo", "build", "--no-such-flag"]);
    assert!(
        stderr.contains("outside this command's explicit selection: util (build-script output)"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("An earlier inputs build failed"),
        "{stderr}"
    );
}

fn untrusted_output(repo: &Repo, package: &str) {
    placed(repo, &format!("{SHARED_TARGET}/debug"));
    unit(
        repo,
        &format!("{SHARED_TARGET}/debug/build/{package}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
}

fn workspace_with_members() -> Repo {
    let repo = Repo::new("audit-invocation-workspace");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        "Cargo.toml",
        "[workspace]\nmembers = [\"x\", \"y\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    for package in ["x", "y"] {
        unit(
            &repo,
            &format!("{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(&repo, &format!("{package}/src/lib.rs"), "");
    }
    repo.git(&["add", "--all"]);
    repo
}

fn workspace_output(repo: &Repo, args: &[&str]) -> String {
    let output = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .env("CARGO_TARGET_DIR", repo.root().join(WORKSPACE_TARGET))
        .current_dir(repo.root())
        .args(["inputs", "build", "--"])
        .args(args)
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const WORKSPACE_TARGET: &str = "target-velnor/workspace";

#[test]
fn a_workspace_build_excluding_one_member_selects_the_others() {
    for exclude in [
        &["--exclude", "x"][..],
        &["--exclude=x"],
        &["--exclude", "x@0.1.0"],
    ] {
        let repo = workspace_with_members();
        placed(&repo, &format!("{WORKSPACE_TARGET}/debug"));
        unit(
            &repo,
            &format!("{WORKSPACE_TARGET}/debug/build/y-f000000000000005/output"),
            "cargo:rerun-if-changed=src/lib.rs\n",
        );
        let mut args = vec!["cargo", "build", "--workspace"];
        args.extend_from_slice(exclude);
        args.push("--no-such-flag");
        let stderr = workspace_output(&repo, &args);
        assert!(
            !stderr.contains("outside this command's explicit selection"),
            "{exclude:?}: {stderr}"
        );
        assert!(
            stderr.contains("build command failed"),
            "{exclude:?}: {stderr}"
        );
    }
}

#[test]
fn a_failed_workspace_build_at_a_root_outside_the_member_directory_is_blamed_in_the_member_build() {
    let repo = Repo::new("audit-invocation-outside-member");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        "root/Cargo.toml",
        "[workspace]\nmembers = [\"../member\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "root/Cargo.lock", "version = 4\n");
    unit(
        &repo,
        "member/Cargo.toml",
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\nworkspace = \"../root\"\n",
    );
    unit(&repo, "member/src/lib.rs", "");
    repo.git(&["add", "--all"]);
    let failed = inputs_build_in(
        &repo,
        "root",
        &["cargo", "build", "--workspace", "--no-such-flag"],
    );
    assert!(!failed.status.success(), "{failed:?}");
    placed(&repo, &format!("{SHARED_TARGET}/debug"));
    unit(
        &repo,
        &format!("{SHARED_TARGET}/debug/build/member-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    let stderr = String::from_utf8_lossy(
        &inputs_build_in(&repo, "member", &["cargo", "build", "--no-such-flag"]).stderr,
    )
    .into_owned();
    assert!(
        stderr.contains("An earlier inputs build failed"),
        "{stderr}"
    );
}
