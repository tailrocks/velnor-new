// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};
use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci")).args(args)
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("CARGO_TERM_COLOR", "never")
        .env("GITHUB_OUTPUT", repo.root().join("outputs"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env("RUSTC", std::path::Path::new(env!("CARGO")).with_file_name("rustc"))
        .current_dir(repo.root()).output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn a_quiet_cargo_configuration_cannot_hide_compilation_from_the_cache_decision() {
    let repo = Repo::new("cache-quiet");
    repo.write("Cargo.toml", "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n");
    repo.write("src/lib.rs", "pub fn leaf() -> u8 { 1 }\n");
    repo.write(".cargo/config.toml", "[term]\nquiet = true\n");
    repo.write_unstaged("temp/velnor-run.log", "");
    success(&helper(&repo, &["run", "--", env!("CARGO"), "build", "--offline"]));
    let output = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&output);
    let written = fs::read_to_string(repo.root().join("outputs")).unwrap_or_default();
    assert!(written.contains("save=true\n"), "{output:?}: {written}");
}
