// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::cache_private_repo::{GIT_LOCK, private_checkout};

const CREDENTIAL: &str = "home/.cargo/git/db/shared-1/.git-credentials";

fn credential_line() -> String {
    let (user, password) = ("x-access-token", "ghs_secret");
    format!("https://{user}:{password}@github.com\n")
}

fn decide(name: &str, restored: Option<&str>) -> (Output, String) {
    let repo = private_checkout(name, GIT_LOCK);
    repo.write_unstaged(CREDENTIAL, &credential_line());
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        temp.join("velnor-run.log"),
        "   Compiling app v0.1.0 (/work/lab/app)\n    Finished `dev` profile in 3.20s\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["cache", "decide", "--target-dir", "target"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .env("HOME", repo.root().join("home"))
        .env("VELNOR_REPOSITORY_PRIVATE", "true")
        .current_dir(repo.root());
    if let Some(restored) = restored {
        command.env("VELNOR_SOURCES_RESTORED", restored);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&outputs).unwrap_or_default())
}

#[test]
fn an_exact_sources_hit_skips_the_credential_scan_that_only_guards_the_sources_save() {
    let (output, written) = decide("decide-sources-hit", Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("credential"),
        "{output:?}"
    );
}

#[test]
fn a_sources_miss_or_no_record_of_the_restore_still_scans_for_credentials() {
    for (label, restored) in [
        ("miss", Some("false")),
        ("empty", Some("")),
        ("unset", None),
    ] {
        let (output, written) = decide(&format!("decide-sources-{label}"), restored);
        assert!(output.status.success(), "{label}: {output:?}");
        assert_eq!(written, "save=true\nsources=false\n", "{label}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("credential"),
            "{label}: {output:?}"
        );
    }
}
