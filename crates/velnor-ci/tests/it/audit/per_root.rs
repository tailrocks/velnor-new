// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;

use super::{audit, placed, unit};

const GROUP_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n")
}

fn package(repo: &Repo, directory: &str, name: &str) {
    unit(repo, &format!("{directory}/Cargo.toml"), &manifest(name));
    unit(
        repo,
        &format!("{directory}/Cargo.lock"),
        &format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    );
    unit(repo, &format!("{directory}/src/lib.rs"), "");
    unit(repo, &format!("{directory}/data.txt"), "");
}

fn built(repo: &Repo, target: &str, name: &str, hashes: [&str; 2]) {
    placed(repo, target);
    for (hash, fingerprint) in hashes.into_iter().zip(["lib", "test-lib"]) {
        unit(
            repo,
            &format!("{target}/deps/{name}-{hash}.d"),
            "out.d: src/lib.rs data.txt\n",
        );
        unit(
            repo,
            &format!("{target}/.fingerprint/{name}-{hash}/{fingerprint}-{name}"),
            "",
        );
    }
}

fn repo(group: bool, web: bool) -> Repo {
    let repo = Repo::new("audit-per-root");
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
    for (directory, name) in [("tools/a", "a"), ("tools/b", "b"), ("web", "w")] {
        package(&repo, directory, name);
    }
    repo.git(&["add", "--all"]);
    if group {
        let shared = "target-velnor/tools/debug";
        built(&repo, shared, "a", ["f000000000000001", "f000000000000002"]);
        built(&repo, shared, "b", ["f000000000000003", "f000000000000004"]);
    }
    if web {
        built(
            &repo,
            "web/target/debug",
            "w",
            ["f000000000000005", "f000000000000006"],
        );
    }
    repo
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_root_in_a_group_audits_the_whole_group_and_needs_no_other_root_built() {
    let repo = repo(true, false);
    let output = audit(&repo, &["--root", "tools/a"]);
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(!text.contains("found no current dep-info"), "{text}");
    assert!(text.contains("tools/a/data.txt"), "{text}");
    assert!(text.contains("tools/b/data.txt"), "{text}");
    assert!(!text.contains("web/data.txt"), "{text}");
}

#[test]
fn a_standalone_root_is_audited_without_the_group_built() {
    let repo = repo(false, true);
    let output = audit(&repo, &["--root", "web"]);
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(!text.contains("found no current dep-info"), "{text}");
    assert!(text.contains("web/data.txt"), "{text}");
    assert!(!text.contains("tools/"), "{text}");
}

#[test]
fn a_per_root_audit_compares_only_the_lock_lines_of_its_own_packages() {
    let repo = repo(true, true);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{}", stderr(&written));
    repo.git(&["add", "--all"]);
    for directory in ["web", "tools/b"] {
        let output = audit(&repo, &["--root", directory]);
        assert!(output.status.success(), "{directory}: {}", stderr(&output));
    }
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let text = fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let without_web: Vec<&str> = text.lines().filter(|line| !line.contains("web/")).collect();
    fs::write(&lock, format!("{}\n", without_web.join("\n")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    let output = audit(&repo, &["--root", "web"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let output = audit(&repo, &["--root", "tools/a"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_per_root_audit_names_the_roots_when_the_directory_is_not_one() {
    let repo = repo(true, true);
    let output = audit(&repo, &["--root", "nowhere"]);
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("no Cargo root in nowhere"), "{text}");
    assert!(text.contains("tools/a"), "{text}");
}

#[test]
fn a_per_root_audit_cannot_write_the_lock() {
    let repo = repo(true, true);
    let output = audit(&repo, &["--root", "web", "--write"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
