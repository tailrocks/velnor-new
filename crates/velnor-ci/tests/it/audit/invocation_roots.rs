// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{command, placed, unit};

const GROUP_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

const SHARED_TARGET: &str = "target-velnor/tools";

fn grouped_tools() -> Repo {
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
    for package in ["a", "b"] {
        unit(
            &repo,
            &format!("tools/{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(
            &repo,
            &format!("tools/{package}/Cargo.lock"),
            &format!("version = 4\n\n[[package]]\nname = \"{package}\"\nversion = \"0.1.0\"\n"),
        );
        unit(&repo, &format!("tools/{package}/src/lib.rs"), "");
        unit(&repo, &format!("tools/{package}/data.txt"), "");
    }
    repo.git(&["add", "--all"]);
    repo
}

fn untrusted_b_output(repo: &Repo) {
    placed(repo, &format!("{SHARED_TARGET}/debug"));
    unit(
        repo,
        &format!("{SHARED_TARGET}/debug/build/b-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
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

fn a_failed_build_in_tools_a_does_not_blame_tools_b_after_it(failing: &[&str]) {
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
