// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::Value;

use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    velnor_test_support::keep_mise_cargo_home(&mut command);
    command
        .args(args)
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("CARGO_TERM_COLOR", "never")
        .env("GITHUB_OUTPUT", repo.root().join("outputs"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

/// The vendored copy of `dep` stands in for a registry crate: the cache decision refuses a
/// checkout whose Cargo configuration replaces a source, so the replacement is a command line option.
fn build(repo: &Repo) {
    let vendor = repo.root().join("vendor");
    for file in ["velnor-run.log", "velnor-invocations.jsonl"] {
        repo.write_unstaged(&format!("temp/{file}"), "");
    }
    let cargo = velnor_test_support::real_cargo_str();
    success(&helper(
        repo,
        &[
            "run",
            "--",
            &cargo,
            "build",
            "--offline",
            "--config",
            "source.crates-io.replace-with='vendored'",
            "--config",
            &format!("source.vendored.directory='{}'", vendor.display()),
            "--message-format=json-render-diagnostics",
        ],
    ));
}

/// Dependency units of the last `build`, as `(package, fresh)`; workspace units always rebuild.
fn dependency_units(repo: &Repo) -> Vec<(String, bool)> {
    let text = fs::read_to_string(repo.root().join("temp/velnor-invocations.jsonl"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut units = Vec::new();
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).unwrap_or_else(|e| unreachable!("{e}"));
        for artifact in record["artifacts"].as_array().into_iter().flatten() {
            let package = artifact["package_id"].as_str().unwrap_or_default();
            if !package.starts_with("path+") {
                units.push((package.to_owned(), artifact["fresh"] == true));
            }
        }
    }
    units
}

#[test]
fn a_pruned_build_directory_leaves_every_dependency_fresh_for_the_next_build() {
    let repo = Repo::new("prune-fresh");
    repo.write(
        "Cargo.toml",
        "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\ndep = '1.0.0'\n[workspace]\n",
    );
    repo.write("src/lib.rs", "pub fn leaf() -> u8 { dep::dep() }\n");
    repo.write(
        "vendor/dep/Cargo.toml",
        "[package]\nname = 'dep'\nversion = '1.0.0'\nedition = '2021'\nbuild = 'build.rs'\n",
    );
    repo.write("vendor/dep/build.rs", "fn main() {}\n");
    repo.write("vendor/dep/src/lib.rs", "pub fn dep() -> u8 { 1 }\n");
    repo.write(
        "vendor/dep/.cargo-checksum.json",
        "{\"files\":{},\"package\":null}\n",
    );
    build(&repo);
    assert!(dependency_units(&repo).iter().all(|(_, fresh)| !fresh));
    let decided = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&decided);
    let stdout = String::from_utf8_lossy(&decided.stdout);
    assert!(stdout.contains("this run's dependency units"), "{stdout}");
    assert!(!repo.root().join("target/debug/libleaf.rlib").exists());
    assert!(!repo.root().join("target/debug/incremental").exists());
    repo.write_unstaged("src/lib.rs", "pub fn leaf() -> u8 { dep::dep() + 1 }\n");
    build(&repo);
    let units = dependency_units(&repo);
    assert_eq!(
        units.len(),
        2,
        "the build script and the library: {units:?}"
    );
    assert!(units.iter().all(|(_, fresh)| *fresh), "{units:?}");
}
