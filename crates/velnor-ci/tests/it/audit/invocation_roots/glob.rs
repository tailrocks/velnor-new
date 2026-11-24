// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::io::Repo;

use super::{WORKSPACE_TARGET, command, placed, unit};

const REFUSAL: &str = "outside this command's explicit selection";
const LONG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LIMIT: Duration = Duration::from_secs(15);

fn workspace_with_long_member() -> Repo {
    let repo = Repo::new("audit-invocation-glob-backtracking");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        "Cargo.toml",
        &format!("[workspace]\nmembers = [\"{LONG}\", \"x\"]\nresolver = \"3\"\n"),
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    for package in [LONG, "x"] {
        unit(
            &repo,
            &format!("{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(&repo, &format!("{package}/src/lib.rs"), "");
    }
    placed(&repo, &format!("{WORKSPACE_TARGET}/debug"));
    unit(
        &repo,
        &format!("{WORKSPACE_TARGET}/debug/build/{LONG}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    repo.git(&["add", "--all"]);
    repo
}

fn stderr_within_limit(selection: &[&str]) -> String {
    let repo = workspace_with_long_member();
    let mut child = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .env("CARGO_TARGET_DIR", repo.root().join(WORKSPACE_TARGET))
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "cargo", "build"])
        .args(selection)
        .arg("--no-such-flag")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .is_some()
        {
            let output = child
                .wait_with_output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            return String::from_utf8_lossy(&output.stderr).into_owned();
        }
        if started.elapsed() > LIMIT {
            child.kill().unwrap_or_else(|e| unreachable!("{e}"));
            child.wait().unwrap_or_else(|e| unreachable!("{e}"));
            unreachable!("matching {selection:?} did not finish within {LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn many_stars(last: &str) -> String {
    format!("{}{last}", "*a".repeat(24))
}

#[test]
fn a_package_glob_with_many_stars_that_cannot_match_is_decided_without_backtracking() {
    let pattern = many_stars("*z");
    for flag in ["-p", "--package"] {
        let stderr = stderr_within_limit(&[flag, &pattern]);
        assert!(stderr.contains(REFUSAL), "{stderr}");
    }
    let stderr = stderr_within_limit(&["--workspace", "--exclude", &pattern]);
    assert!(
        !stderr.contains(REFUSAL) && stderr.contains("build command failed"),
        "{stderr}"
    );
}

#[test]
fn a_package_glob_with_many_stars_that_matches_selects_the_member() {
    let pattern = many_stars("*");
    let stderr = stderr_within_limit(&["-p", &pattern]);
    assert!(
        !stderr.contains(REFUSAL) && stderr.contains("build command failed"),
        "{stderr}"
    );
    let stderr = stderr_within_limit(&["--workspace", "--exclude", &pattern]);
    assert!(stderr.contains(REFUSAL), "{stderr}");
}
