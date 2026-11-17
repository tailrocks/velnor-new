// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_ci::io::{CaseSensitivity, check_input_target_with, cold_on_failure, restore_inputs};

use crate::io::Repo;
use crate::support::fresh_dir;

#[test]
fn checkout_under_a_cargo_ancestor_still_saves_its_default_target() {
    let parent = fresh_dir("velnor-ci-cargo-ancestor", "parent");
    let cargo = parent.join(".cargo");
    fs::create_dir(&cargo).unwrap_or_else(|error| unreachable!("{error}"));
    let repo = Repo::new_under(&cargo, "project");
    repo.write(
        "Cargo.toml",
        "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n",
    );
    repo.write("src/lib.rs", "pub fn leaf() -> u8 { 1 }\n");
    let build = Command::new(env!("CARGO"))
        .args(["build", "--offline"])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(build.status.success(), "{build:?}");
    let save = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", "save"])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(save.status.success(), "{save:?}");
    assert!(repo.root().join("target/.velnor/inputs.tsv").is_file());
    drop(repo);
    fs::remove_dir_all(parent).unwrap_or_else(|error| unreachable!("{error}"));
}

fn case_alias(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("src/lib.rs", "checkout\n");
    repo.write_unstaged("target/keep", "tracked data\n");
    repo.write_unstaged("target/.velnor/inputs.tsv", "invalid\n");
    repo.write_unstaged("target/cache", "cached data\n");
    let blob = Command::new("git")
        .args(["hash-object", "-w", "target/keep"])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(blob.status.success(), "{blob:?}");
    let blob = String::from_utf8(blob.stdout).unwrap_or_else(|error| unreachable!("{error}"));
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{},Target/keep", blob.trim()),
    ]);
    repo
}

fn restore(repo: &Repo, sensitivity: CaseSensitivity) -> Result<String, velnor_ci::error::CiError> {
    let target = Path::new("target");
    check_input_target_with(repo.root(), target, &|_| Ok(sensitivity))?;
    cold_on_failure(&[repo.root().join(target)], "inputs restore", || {
        restore_inputs(repo.root(), target)
    })
}

#[test]
fn insensitive_case_alias_refuses_cold_cleanup_and_preserves_tracked_data() {
    let repo = case_alias("case-alias");
    let result = restore(&repo, CaseSensitivity::Insensitive);
    assert!(result.is_err(), "{result:?}");
    let error = result
        .err()
        .unwrap_or_else(|| unreachable!("restore was accepted"));
    assert!(error.to_string().contains("contains checkout files"));
    for (path, expected) in [
        ("target/keep", "tracked data\n"),
        ("target/cache", "cached data\n"),
        ("target/.velnor/inputs.tsv", "invalid\n"),
        ("src/lib.rs", "checkout\n"),
    ] {
        assert_eq!(
            fs::read_to_string(repo.root().join(path)).unwrap_or_default(),
            expected
        );
    }
    repo.git(&["ls-files", "--error-unmatch", "Target/keep"]);
}

#[test]
fn sensitive_case_names_are_distinct_and_safe_targets_build_cold() {
    let repo = case_alias("case-distinct");
    check_input_target_with(repo.root(), Path::new("target"), &|_| {
        Ok(CaseSensitivity::Sensitive)
    })
    .unwrap_or_else(|error| unreachable!("{error}"));
    let safe = Repo::new("case-safe");
    safe.write("src/lib.rs", "checkout\n");
    for name in ["target", "缓存"] {
        safe.write_unstaged(&format!("{name}/.velnor/inputs.tsv"), "invalid\n");
        let target = Path::new(name);
        check_input_target_with(safe.root(), target, &|_| Ok(CaseSensitivity::Insensitive))
            .unwrap_or_else(|error| unreachable!("{error}"));
        let result = cold_on_failure(&[safe.root().join(target)], "inputs restore", || {
            restore_inputs(safe.root(), target)
        })
        .unwrap_or_else(|error| unreachable!("{error}"));
        assert!(result.contains("building cold"), "{result}");
        assert!(!safe.root().join(target).exists());
    }
    assert_eq!(
        fs::read_to_string(safe.root().join("src/lib.rs")).unwrap_or_default(),
        "checkout\n"
    );
}

#[test]
fn runtime_probe_refuses_case_alias_on_an_insensitive_filesystem() {
    let repo = case_alias("case-runtime");
    if !repo.root().join("TARGET/KEEP").exists() {
        return;
    }
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", "restore"])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("contains checkout files"),
        "{output:?}"
    );
    assert_eq!(
        fs::read_to_string(repo.root().join("target/keep")).unwrap_or_default(),
        "tracked data\n"
    );
    assert_eq!(
        fs::read_to_string(repo.root().join("target/.velnor/inputs.tsv")).unwrap_or_default(),
        "invalid\n"
    );
}
