// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::mbx_store::{
    Inputs, KEEP_OUT_DIR, Outcome, env_lines, message, select, shims_line, trusted_event,
};

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

fn run(event: &str, git_ref: &str, shared: Option<&Path>) -> Outcome {
    let shared = shared.map(|path| path.display().to_string());
    select(&Inputs {
        event_name: event,
        git_ref,
        ref_protected: true,
        default_branch: Some("main"),
        shared_dir: shared.as_deref(),
    })
}

fn shims(host: &Host) -> PathBuf {
    host.root.join("temp/velnor-mbx-shims")
}

fn lines(host: &Host, outcome: &Outcome) -> String {
    env_lines(outcome, &host.root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn trust_follows_the_event_and_the_default_branch() {
    let cases = [
        ("push", "refs/heads/main", true),
        ("schedule", "refs/heads/main", true),
        (
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1-a",
            false,
        ),
        ("push", "refs/heads/feature", false),
        ("push", "refs/heads/MAIN", false),
        ("push", "refs/heads/Main", false),
        ("workflow_dispatch", "refs/heads/MAIN", false),
        ("schedule", "refs/heads/MAIN", false),
        ("push", "refs/tags/main", false),
        ("pull_request", "refs/pull/1/merge", false),
        ("pull_request_target", "refs/heads/main", false),
        ("workflow_dispatch", "refs/heads/main", true),
        ("workflow_dispatch", "refs/heads/feature", false),
        ("", "", false),
    ];
    for (event, git_ref, expected) in cases {
        assert_eq!(
            trusted_event(event, git_ref, true, Some("main")),
            expected,
            "{event} {git_ref}"
        );
    }
    assert!(!trusted_event("push", "refs/heads/main", true, None));
    assert!(!trusted_event("push", "refs/heads/main", true, Some("")));
    assert!(!trusted_event(
        "push",
        "refs/heads/MAIN",
        true,
        Some("main")
    ));
    assert!(!trusted_event(
        "push",
        "refs/heads/main",
        true,
        Some("MAIN")
    ));
}

#[test]
fn a_trusted_event_uses_the_shared_directory_with_pod_private_shims() {
    let host = host("mbx-store-trusted");
    let shared = host.root.join("shared");
    for (event, git_ref) in [("push", "refs/heads/main"), ("schedule", "refs/heads/main")] {
        let outcome = run(event, git_ref, Some(&shared));
        assert_eq!(
            lines(&host, &outcome),
            format!(
                "MBX_CACHE_DIR={}\nMBX_SHIMS_DIR={}\nMBX_SHARE_OUT_DIR=0\n",
                shared.display(),
                shims(&host).display()
            ),
            "{event}"
        );
    }
    let mode = fs::metadata(shims(&host)).map_or_default(|m| m.permissions().mode() & 0o777);
    assert_eq!(mode, 0o700);
    let leftovers: Vec<_> = fs::read_dir(&shared)
        .map(Iterator::count)
        .into_iter()
        .collect();
    assert_eq!(
        leftovers,
        [0],
        "the writability probe leaves nothing behind"
    );
}

#[test]
fn a_pull_request_never_gets_the_shared_directory_but_the_same_shims() {
    let host = host("mbx-store-pr");
    let outcome = run(
        "pull_request",
        "refs/pull/1/merge",
        Some(&host.root.join("shared")),
    );
    assert_eq!(outcome, Outcome::Untrusted);
    assert_eq!(
        lines(&host, &outcome),
        format!("MBX_SHIMS_DIR={}\n{KEEP_OUT_DIR}", shims(&host).display())
    );
}

#[test]
fn a_merge_group_never_gets_the_shared_directory_but_the_same_shims() {
    let host = host("mbx-store-queue");
    let outcome = run(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1-a",
        Some(&host.root.join("shared")),
    );
    assert_eq!(outcome, Outcome::Untrusted);
    assert_eq!(
        lines(&host, &outcome),
        format!("MBX_SHIMS_DIR={}\n{KEEP_OUT_DIR}", shims(&host).display())
    );
}

#[test]
fn a_stale_probe_file_does_not_block_the_shared_directory() {
    let host = host("mbx-store-stale");
    let shared = host.root.join("shared");
    let stale = shared.join(format!(".velnor-probe-{}", std::process::id()));
    fs::write(&stale, "").unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = run("push", "refs/heads/main", Some(&shared));
    assert!(matches!(outcome, Outcome::Shared { .. }), "{outcome:?}");
    assert!(stale.exists());
}

#[test]
fn no_offered_directory_appends_only_the_out_dir_setting() {
    let host = host("mbx-store-none");
    let outcome = run("push", "refs/heads/main", None);
    assert_eq!(outcome, Outcome::NotOffered);
    assert_eq!(lines(&host, &outcome), KEEP_OUT_DIR);
    assert!(!shims(&host).exists());
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
    let root_user = fs::File::create(root.join("ro").join("probe")).is_ok();
    if root_user {
        fs::remove_file(root.join("ro").join("probe")).ok();
    }
    let mut cases: Vec<(PathBuf, &str)> = vec![
        (PathBuf::from("relative/dir"), "not absolute"),
        (root.join("missing"), "does not exist"),
        (root.join("file"), "not a directory"),
        (root.join("link"), "is a symlink"),
        (root.join("via/inner"), "path contains a symlink"),
        (root.join("shared/../shared"), "not a normal path"),
    ];
    if !root_user {
        cases.push((root.join("ro"), "not writable"));
    }
    for (dir, reason) in cases {
        let outcome = run("push", "refs/heads/main", Some(&dir));
        assert_eq!(
            outcome,
            Outcome::Rejected(reason_static(reason)),
            "{}",
            dir.display()
        );
        assert_eq!(
            lines(&host, &outcome),
            format!("MBX_SHIMS_DIR={}\n{KEEP_OUT_DIR}", shims(&host).display()),
            "{}",
            dir.display()
        );
        assert!(message(&outcome).starts_with("::warning::VELNOR_MBX_SHARED_DIR ignored"));
    }
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

#[test]
fn an_existing_shims_directory_is_secured_or_refused() {
    let host = host("mbx-store-shims");
    let temp = host.root.join("temp");
    let shims = shims(&host);
    fs::create_dir(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&shims, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        shims_line(&temp).as_deref().ok(),
        Some(format!("MBX_SHIMS_DIR={}\n", shims.display()).as_str())
    );
    let mode = fs::metadata(&shims)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o700);

    fs::remove_dir(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    let target = host.root.join("elsewhere");
    fs::create_dir(&target).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, &shims).unwrap_or_else(|e| unreachable!("{e}"));
    let error = shims_line(&temp).err().map_or_default(|e| e.to_string());
    assert!(error.contains("shims path is not a directory"), "{error}");
}

#[test]
fn a_store_or_shims_directory_with_a_line_break_cannot_add_an_environment_variable() {
    let host = host("mbx-store-line-break");
    let temp = host.root.join("temp");
    let clean = Outcome::Shared {
        dir: PathBuf::from("/runner/mbx"),
    };
    assert_eq!(
        env_lines(&clean, &temp).as_deref().ok(),
        Some(
            format!(
                "MBX_CACHE_DIR=/runner/mbx\nMBX_SHIMS_DIR={}\nMBX_SHARE_OUT_DIR=0\n",
                shims(&host).display()
            )
            .as_str()
        )
    );
    let outcome = Outcome::Shared {
        dir: PathBuf::from("/runner/mbx\nGITHUB_PATH=/evil"),
    };
    assert!(env_lines(&outcome, &temp).is_err());
    let broken = host.root.join("te\nmp");
    fs::create_dir(&broken).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(shims_line(&broken).is_err());
}

#[test]
fn the_command_appends_the_out_dir_setting_to_github_env_whatever_the_event() {
    for (name, event, git_ref) in [
        ("mbx-store-env-push", "push", "refs/heads/main"),
        ("mbx-store-env-pr", "pull_request", "refs/pull/1/merge"),
    ] {
        let host = host(name);
        let env_file = host.root.join("github-env");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .arg("mbx-store")
            .env_clear()
            .env("RUNNER_TEMP", host.root.join("temp"))
            .env("GITHUB_ENV", &env_file)
            .env("GITHUB_EVENT_NAME", event)
            .env("GITHUB_REF", git_ref)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(
            fs::read_to_string(&env_file).unwrap_or_default(),
            KEEP_OUT_DIR,
            "{name}"
        );
    }
}

#[test]
fn the_command_gives_a_default_branch_push_the_shared_store_only_when_its_ref_is_protected() {
    for (name, protected, shared) in [
        ("mbx-store-protected", Some("true"), true),
        ("mbx-store-unprotected", Some("false"), false),
        ("mbx-store-protection-unknown", None, false),
    ] {
        let host = host(name);
        let env_file = host.root.join("github-env");
        let event_file = host.root.join("event.json");
        fs::write(&event_file, r#"{"repository":{"default_branch":"main"}}"#)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .arg("mbx-store")
            .env_clear()
            .env("RUNNER_TEMP", host.root.join("temp"))
            .env("GITHUB_ENV", &env_file)
            .env("GITHUB_EVENT_PATH", &event_file)
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF", "refs/heads/main")
            .env("VELNOR_MBX_SHARED_DIR", host.root.join("shared"));
        if let Some(value) = protected {
            command.env("GITHUB_REF_PROTECTED", value);
        }
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{name}: {output:?}");
        let lines = fs::read_to_string(&env_file).unwrap_or_default();
        assert_eq!(lines.contains("MBX_CACHE_DIR="), shared, "{name}: {lines}");
        assert!(lines.ends_with(KEEP_OUT_DIR), "{name}: {lines}");
    }
}
