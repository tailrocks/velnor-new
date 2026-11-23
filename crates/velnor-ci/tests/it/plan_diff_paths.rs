// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::io::Repo;
use crate::plan_diff::{aside, read};

fn head_of(repo: &Repo) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn route(job: &str, dir: &str) -> serde_json::Value {
    serde_json::json!({
        "job_id": job,
        "logical_job_id": job,
        "runner": "scale-set:velnor+ubuntu-26.04-scale-set",
        "target": "x86_64-unknown-linux-gnu",
        "task_ids": [format!("stack/rust/{dir}/build/default")],
    })
}

fn package(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n")
}

fn workspace(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_all(&[
        (".velnor/config.toml", "schema = 2\n"),
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\", \"b\", \"pk\\nx\"]\nresolver = \"3\"\n",
        ),
        ("a/Cargo.toml", &package("a")),
        ("a/src/lib.rs", ""),
        ("b/Cargo.toml", &package("b")),
        ("b/src/lib.rs", ""),
        ("pk\nx/Cargo.toml", &package("pkx")),
        ("pk\nx/src/lib.rs", ""),
    ]);
    repo.commit();
    repo
}

fn plan_changing_a(repo: &Repo, routed_dir: &str, tools: Option<&Path>) -> (Output, String) {
    let base = head_of(repo);
    repo.write("a/src/lib.rs", "pub fn changed() {}\n");
    repo.commit();
    let request = serde_json::json!({
        "base": base,
        "event": "pull_request",
        "head": head_of(repo),
        "lane_inventory": {
            "mode": null,
            "routes": [route("rust-a", "a"), route("rust-b", "b"), route("rust-pkx", routed_dir)],
        },
        "op": "plan-v1",
        "repository": "tailrocks/velnor-new",
        "root": ".",
        "schema": 1,
    });
    let file = aside(repo.root()).join("plan-request.json");
    fs::write(&file, request.to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = aside(repo.root()).join("outputs");
    let mut path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(dir) = tools {
        let mut prefixed = dir.as_os_str().to_owned();
        prefixed.push(":");
        prefixed.push(&path);
        path = prefixed;
    }
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env_clear()
        .env("PATH", path)
        .env("HOME", repo.root())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_RUN_ID", "44491127982")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .args(["plan", "--mode", "ci", "--request"])
        .arg(&file)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = if outputs.exists() {
        read(&outputs)
    } else {
        String::new()
    };
    fs::remove_dir_all(aside(repo.root())).unwrap_or_else(|e| unreachable!("{e}"));
    (output, written)
}

#[test]
fn a_routed_package_under_a_newline_directory_selects_only_its_own_jobs() {
    let repo = workspace("diff-newline-directory");
    let (output, written) = plan_changing_a(&repo, "pk\nx", None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        written.contains("\nnot_affected_jobs=,rust-b,rust-pkx,\n"),
        "{written} {stderr}"
    );
    assert!(!stderr.contains("all_changed"), "{stderr}");
}

#[test]
fn a_routed_directory_holding_a_nul_byte_never_reaches_the_object_reader() {
    let repo = workspace("diff-nul-directory");
    let (output, written) = plan_changing_a(&repo, "pk\u{0}x", None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(written.contains("\nnot_affected_jobs=\n"), "{written}");
    assert!(stderr.contains("NUL"), "{stderr}");
    assert!(!stderr.contains("cat-file"), "{stderr}");
}

fn tools_without_nul_framing() -> PathBuf {
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("git"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| unreachable!("git on PATH"));
    let dir = crate::support::fresh_dir("velnor-ci-old-git", "bin");
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in\n  *\" cat-file -Z \"*)\n    echo \"error: unknown switch \\`Z'\" >&2\n    exit 129\n    ;;\nesac\nexec '{}' \"$@\"\n",
        real.display()
    );
    let fake = dir.join("git");
    fs::write(&fake, script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

#[test]
fn a_git_without_nul_framed_cat_file_runs_every_job_with_a_stable_reason() {
    let repo = workspace("diff-old-git");
    let bin = tools_without_nul_framing();
    let (output, written) = plan_changing_a(&repo, "pk\nx", Some(&bin));
    fs::remove_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(written.contains("\nnot_affected_jobs=\n"), "{written}");
    assert!(stderr.contains("git_too_old:all_changed"), "{stderr}");
    assert!(!stderr.contains("unknown switch"), "{stderr}");
}
