// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;
use velnor_test_support::{ci_safe_cargo_env, real_cargo_str};

const DEBUG: &str = "rust/target/debug";

fn refused_with(output: &Output, texts: &[&str]) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for text in texts {
        assert!(stderr.contains(text), "missing {text:?} in {stderr}");
    }
}

/// The unit hashes of the `lib` build-script runs with an output under `target/debug`.
fn lib_runs(repo: &Repo) -> BTreeSet<String> {
    fs::read_dir(repo.root().join(DEBUG).join("build"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")))
        .filter(|entry| entry.path().join("output").is_file())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_prefix("lib-")
                .map(str::to_owned)
        })
        .collect()
}

fn build_step(repo: &Repo, cargo: &[&str]) -> Output {
    let mut command = velnor_ci();
    ci_safe_cargo_env(&mut command)
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", &real_cargo_str()])
        .args(cargo)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn library_with_asset_input() -> Repo {
    let repo = Repo::new("stale-run");
    repo.write_all(&[
        ("mise.toml", "[tools]\n"),
        (
            "rust/Cargo.toml",
            "[workspace]\nmembers = [\"lib\"]\nresolver = \"3\"\n",
        ),
        ("rust/Cargo.lock", "version = 4\n"),
        (
            "rust/lib/Cargo.toml",
            "[package]\nname = \"lib\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
        ),
        ("rust/lib/src/lib.rs", "pub fn value() -> u32 { 1 }\n"),
        (
            "rust/lib/build.rs",
            "fn main() { println!(\"cargo:rerun-if-changed=asset.txt\"); }\n",
        ),
        ("rust/lib/asset.txt", "first\n"),
    ]);
    repo
}

#[test]
fn an_input_changed_between_two_steps_names_the_input_and_the_stale_unit() {
    let repo = library_with_asset_input();
    let clippy = build_step(&repo, &["clippy", "--workspace", "--all-targets"]);
    assert!(clippy.status.success(), "{clippy:?}");
    let before = lib_runs(&repo);
    assert_eq!(before.len(), 1, "{before:?}");

    repo.write_unstaged("rust/lib/asset.txt", "second\n");
    repo.set_mtime(
        "rust/lib/asset.txt",
        SystemTime::now() + Duration::from_secs(100),
    );
    let build = build_step(&repo, &["build", "--workspace", "--all-targets"]);
    assert!(build.status.success(), "{build:?}");
    let after = lib_runs(&repo);
    assert_eq!(after.len(), 2, "{after:?}");
    let stale = before.iter().next().unwrap_or_else(|| unreachable!());

    refused_with(
        &audit(&repo, &["--write"]),
        &[&format!(
            "output stale: input asset.txt changed after it ran (unit {stale}); produce build-script inputs before the first compile step, e.g. [[test_setup]] phase = \"build\""
        )],
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_run_whose_output_is_older_than_its_build_script_says_so() {
    let repo = workspace_below_the_root();
    own_units(&repo, DEBUG, &[]);
    unit(
        &repo,
        &format!("{DEBUG}/.fingerprint/lib-f0000000000000e3/run-build-script-build-script-build"),
        "",
    );
    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000e3/output"),
        "",
    );
    repo.set_mtime(
        &format!("{DEBUG}/build/lib-f0000000000000e3/output"),
        UNIX_EPOCH + Duration::from_secs(1),
    );
    refused_with(
        &audit(&repo, &["--write"]),
        &[
            "/rust/target/debug for lib target  (output older than its build script (unit f0000000000000e3));",
        ],
    );
}
