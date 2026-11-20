// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use serde_json::Value;

use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("CARGO_TERM_COLOR", "never")
        .env("GITHUB_OUTPUT", repo.root().join("outputs"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env(
            "RUSTC",
            std::path::Path::new(env!("CARGO")).with_file_name("rustc"),
        )
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

fn build(repo: &Repo) {
    for file in ["velnor-run.log", "velnor-invocations.jsonl"] {
        repo.write_unstaged(&format!("temp/{file}"), "");
    }
    success(&helper(
        repo,
        &["run", "--", env!("CARGO"), "build", "--offline"],
    ));
}

/// Dependency units cargo rebuilt in the last `build`: workspace units always rebuild.
fn rebuilt_dependencies(repo: &Repo) -> Vec<String> {
    let text = fs::read_to_string(repo.root().join("temp/velnor-invocations.jsonl"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut rebuilt = Vec::new();
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).unwrap_or_else(|e| unreachable!("{e}"));
        for artifact in record["artifacts"].as_array().into_iter().flatten() {
            let package = artifact["package_id"].as_str().unwrap_or_default();
            if artifact["fresh"] == false && !package.starts_with("path+") {
                rebuilt.push(package.to_owned());
            }
        }
    }
    rebuilt
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
        ".cargo/config.toml",
        "[source.crates-io]\nreplace-with = 'vendored'\n[source.vendored]\ndirectory = 'vendor'\n",
    );
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
    assert!(!rebuilt_dependencies(&repo).is_empty());
    let decided = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&decided);
    let stdout = String::from_utf8_lossy(&decided.stdout);
    assert!(stdout.contains("this run's dependency units"), "{stdout}");
    assert!(!repo.root().join("target/debug/libleaf.rlib").exists());
    assert!(!repo.root().join("target/debug/incremental").exists());
    repo.write_unstaged("src/lib.rs", "pub fn leaf() -> u8 { dep::dep() + 1 }\n");
    build(&repo);
    assert_eq!(rebuilt_dependencies(&repo), Vec::<String>::new());
    let log = fs::read_to_string(repo.root().join("temp/velnor-run.log")).unwrap_or_default();
    assert!(log.contains("Compiling leaf"), "{log}");
    assert!(!log.contains("Compiling dep"), "{log}");
}
