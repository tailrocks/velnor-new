// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::mbx_store::{Inputs, Outcome, env_lines, message, select, trusted_event};

use crate::io::Repo;

struct Host {
    _repo: Repo,
    root: PathBuf,
}

fn host(name: &str) -> Host {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"));
    Host { _repo: repo, root }
}

fn run(host: &Host, event: &str, git_ref: &str, shared: Option<&Path>) -> Outcome {
    let shared = shared.map(|path| path.display().to_string());
    let temp = host.root.join("temp");
    select(&Inputs {
        event_name: event,
        git_ref,
        default_branch: Some("main"),
        shared_dir: shared.as_deref(),
        runner_temp: &temp,
    })
}

#[test]
fn trust_follows_the_event_and_the_default_branch() {
    let cases = [
        ("push", "refs/heads/main", true),
        ("schedule", "refs/heads/main", true),
        (
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1-a",
            true,
        ),
        ("push", "refs/heads/feature", false),
        ("push", "refs/tags/main", false),
        ("pull_request", "refs/pull/1/merge", false),
        ("pull_request_target", "refs/heads/main", false),
        ("workflow_dispatch", "refs/heads/main", false),
        ("", "", false),
    ];
    for (event, git_ref, expected) in cases {
        assert_eq!(
            trusted_event(event, git_ref, Some("main")),
            expected,
            "{event} {git_ref}"
        );
    }
    assert!(!trusted_event("push", "refs/heads/main", None));
}

#[test]
fn a_trusted_event_uses_the_shared_directory_with_pod_private_shims() {
    let host = host("mbx-store-trusted");
    let shared = host.root.join("shared");
    for (event, git_ref) in [("push", "refs/heads/main"), ("merge_group", "refs/heads/q")] {
        let outcome = run(&host, event, git_ref, Some(&shared));
        assert_eq!(
            env_lines(&outcome),
            format!(
                "MBX_CACHE_DIR={}\nMBX_SHIMS_DIR={}\n",
                shared.display(),
                host.root.join("temp/velnor-mbx-shims").display()
            ),
            "{event}"
        );
    }
    let mode = fs::metadata(host.root.join("temp/velnor-mbx-shims"))
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or_default();
    assert_eq!(mode, 0o700);
    let leftovers: Vec<_> = fs::read_dir(&shared)
        .map(|d| d.count())
        .into_iter()
        .collect();
    assert_eq!(
        leftovers,
        [0],
        "the writability probe leaves nothing behind"
    );
}

#[test]
fn a_pull_request_never_gets_the_shared_directory() {
    let host = host("mbx-store-pr");
    let outcome = run(
        &host,
        "pull_request",
        "refs/pull/1/merge",
        Some(&host.root.join("shared")),
    );
    assert_eq!(outcome, Outcome::Untrusted);
    assert_eq!(env_lines(&outcome), "");
    assert!(!host.root.join("temp/velnor-mbx-shims").exists());
}

#[test]
fn no_offered_directory_changes_nothing() {
    let host = host("mbx-store-none");
    let outcome = run(&host, "push", "refs/heads/main", None);
    assert_eq!(outcome, Outcome::NotOffered);
    assert_eq!(env_lines(&outcome), "");
}

#[test]
fn an_unusable_directory_warns_and_keeps_the_private_store() {
    let host = host("mbx-store-reject");
    let root = &host.root;
    fs::write(root.join("file"), "").unwrap_or_else(|e| unreachable!("{e}"));
    symlink(root.join("shared"), root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("shared/inner")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(root.join("shared"), root.join("via")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("ro")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(root.join("ro"), fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let cases: [(PathBuf, &str); 7] = [
        (PathBuf::from("relative/dir"), "not absolute"),
        (root.join("missing"), "does not exist"),
        (root.join("file"), "not a directory"),
        (root.join("link"), "is a symlink"),
        (root.join("via/inner"), "path contains a symlink"),
        (root.join("shared/../shared"), "not a normal path"),
        (root.join("ro"), "not writable"),
    ];
    for (dir, reason) in cases {
        let outcome = run(&host, "push", "refs/heads/main", Some(&dir));
        if reason == "not writable" && fs::File::create(dir.join("probe")).is_ok() {
            continue; // running as root: permission bits do not bind
        }
        assert_eq!(
            outcome,
            Outcome::Rejected(reason_static(reason)),
            "{}",
            dir.display()
        );
        assert_eq!(env_lines(&outcome), "");
        assert!(message(&outcome).starts_with("::warning::VELNOR_MBX_SHARED_DIR ignored"));
    }
    assert!(!root.join("temp/velnor-mbx-shims").exists());
}

fn reason_static(reason: &str) -> &'static str {
    [
        "not absolute",
        "does not exist",
        "not a directory",
        "is a symlink",
        "path contains a symlink",
        "not a normal path",
        "not writable",
    ]
    .into_iter()
    .find(|candidate| *candidate == reason)
    .unwrap_or("")
}
