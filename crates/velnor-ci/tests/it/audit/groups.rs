// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;
use crate::support::fresh_dir;

use super::{placed, unit};

const GROUP_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

fn grouped_tools() -> Repo {
    grouped_tools_in(Repo::new("audit-groups"))
}

fn grouped_tools_in(repo: Repo) -> Repo {
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

fn group_units(repo: &Repo, package: &str, normal: &str, harness: &str) {
    let target = "target-velnor/tools/debug";
    for (hash, fingerprint) in [
        (normal, format!("lib-{package}")),
        (harness, format!("test-lib-{package}")),
    ] {
        unit(
            repo,
            &format!("{target}/deps/{package}-{hash}.d"),
            "out.d: src/lib.rs data.txt\n",
        );
        unit(
            repo,
            &format!("{target}/.fingerprint/{package}-{hash}/{fingerprint}"),
            "",
        );
    }
    placed(repo, target);
}

fn audit(repo: &Repo, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(repo.root())
        .args(["inputs", "audit"])
        .args(args);
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn both_roots() -> Repo {
    let repo = grouped_tools();
    group_units(&repo, "a", "f000000000000001", "f000000000000002");
    group_units(&repo, "b", "f000000000000003", "f000000000000004");
    repo
}

#[test]
fn a_grouped_build_is_audited_from_the_shared_target_directory() {
    let repo = both_roots();
    let written = audit(&repo, &["--write"]);
    assert!(
        written.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&written.stderr)
    );
    let lock = fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(lock.contains("tools/a/data.txt"), "{lock}");
    assert!(lock.contains("tools/b/data.txt"), "{lock}");
    repo.git(&["add", "--all"]);
    let compared = audit(&repo, &[]);
    assert!(
        compared.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&compared.stderr)
    );
}

#[test]
fn a_grouped_root_still_needs_its_own_units() {
    let repo = grouped_tools();
    group_units(&repo, "b", "f000000000000003", "f000000000000004");
    let output = audit(&repo, &["--write"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("target-velnor/tools/debug"), "{stderr}");
    assert!(
        stderr.contains("target a (tools/a/src/lib.rs, 0 of 2 units)"),
        "{stderr}"
    );
    assert!(!stderr.contains("target b (tools/b/src/lib.rs"), "{stderr}");
}

#[test]
fn an_ungrouped_root_keeps_its_own_target_directory() {
    let repo = both_roots();
    unit(
        &repo,
        "web/Cargo.toml",
        "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    unit(
        &repo,
        "web/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"w\"\nversion = \"0.1.0\"\n",
    );
    unit(&repo, "web/src/lib.rs", "");
    unit(&repo, "web/data.txt", "");
    repo.git(&["add", "--all"]);
    let target = "web/target/debug";
    placed(&repo, target);
    for (hash, fingerprint) in [
        ("f000000000000005", "lib-w"),
        ("f000000000000006", "test-lib-w"),
    ] {
        unit(
            &repo,
            &format!("{target}/deps/w-{hash}.d"),
            "out.d: src/lib.rs data.txt\n",
        );
        unit(
            &repo,
            &format!("{target}/.fingerprint/w-{hash}/{fingerprint}"),
            "",
        );
    }
    let written = audit(&repo, &["--write"]);
    assert!(
        written.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&written.stderr)
    );
    let lock = fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for source in ["tools/a/data.txt", "tools/b/data.txt", "web/data.txt"] {
        assert!(lock.contains(source), "{source}: {lock}");
    }
}

fn build_grouped(root: &std::path::Path, manifest: &str) -> Output {
    build_grouped_with(root, manifest, false)
}

fn build_grouped_with(root: &std::path::Path, manifest: &str, build_directory: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.env("CARGO_TARGET_DIR", root.join("target-velnor/tools"));
    if build_directory {
        command.env("CARGO_BUILD_BUILD_DIR", root.join("target-velnor/tools"));
    }
    command
        .current_dir(root)
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "build",
            "--manifest-path",
            manifest,
            "--workspace",
            "--all-targets",
            "--locked",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_real_grouped_build_passes_the_audit() {
    assert_real_grouped_build_passes(&grouped_tools());
}

/// A repository reached through a symlink, as on macOS where `$TMPDIR` is
/// `/var/folders/...` and `/var` links to `/private/var`. Cargo and git report
/// the resolved path, so a build that takes the link path must still audit.
#[test]
fn a_real_grouped_build_passes_the_audit_through_a_symlinked_root() {
    let real = fs::canonicalize(fresh_dir("velnor-ci-audit-real", "parent"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let link = real.with_file_name(format!(
        "{}-link",
        real.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| unreachable!())
    ));
    std::os::unix::fs::symlink(&real, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let repo = Repo::new_under(&link, "audit-groups");
    assert_real_grouped_build_passes(&grouped_tools_in(repo));
    let _ignored = fs::remove_file(&link);
    let _ignored = fs::remove_dir_all(&real);
}

fn assert_real_grouped_build_passes(repo: &Repo) {
    unit(
        repo,
        "tools/a/build.rs",
        "fn main() {\n    println!(\"cargo:rerun-if-changed=data.txt\");\n}\n",
    );
    repo.git(&["add", "--all"]);
    for manifest in ["tools/a/Cargo.toml", "tools/b/Cargo.toml"] {
        let built = build_grouped(repo.root(), manifest);
        assert!(
            built.status.success(),
            "{manifest}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
    }
    let written = audit(repo, &["--write"]);
    assert!(
        written.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&written.stderr)
    );
    repo.git(&["add", "--all"]);
    let output = audit(repo, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("lists every read"), "{stdout}");
}

#[test]
fn a_grouped_build_does_not_pull_an_ungrouped_root_into_the_shared_directory() {
    ungrouped_root_beside_the_group(false);
}

#[test]
fn a_build_directory_equal_to_the_shared_target_does_not_refuse_an_ungrouped_root() {
    ungrouped_root_beside_the_group(true);
}

fn ungrouped_root_beside_the_group(build_directory: bool) {
    let repo = grouped_tools();
    unit(
        &repo,
        "web/Cargo.toml",
        "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    unit(
        &repo,
        "web/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"w\"\nversion = \"0.1.0\"\n",
    );
    unit(&repo, "web/src/lib.rs", "");
    repo.git(&["add", "--all"]);
    let built = build_grouped_with(repo.root(), "tools/a/Cargo.toml", build_directory);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
}
