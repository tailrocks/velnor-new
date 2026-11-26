// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::io::Repo;

struct Run {
    _repo: Repo,
    root: PathBuf,
    env_file: PathBuf,
    output: std::process::Output,
}

fn run_binary(name: &str, event: &str, git_ref: &str, prepare: impl FnOnce(&Path)) -> Run {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"));
    prepare(&root.join("temp"));
    let env_file = root.join("github-env");
    let event_file = root.join("event.json");
    fs::write(&event_file, r#"{"repository":{"default_branch":"main"}}"#)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("mbx-store")
        .env_clear()
        .env("RUNNER_TEMP", root.join("temp"))
        .env("GITHUB_ENV", &env_file)
        .env("GITHUB_EVENT_PATH", &event_file)
        .env("GITHUB_EVENT_NAME", event)
        .env("GITHUB_REF", git_ref)
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_MBX_SHARED_DIR", root.join("shared"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    Run {
        _repo: repo,
        root,
        env_file,
        output,
    }
}

fn github_env(name: &str, event: &str, git_ref: &str) -> BTreeMap<String, String> {
    let run = run_binary(name, event, git_ref, |_| {});
    assert!(run.output.status.success(), "{name}: {:?}", run.output);
    fs::read_to_string(&run.env_file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.replace(run.root.to_str().unwrap_or_default(), "<host>");
            (key.to_owned(), value)
        })
        .collect()
}

#[test]
fn pr_and_push_share_build_env() {
    let push = github_env("mbx-store-parity-push", "push", "refs/heads/main");
    for (name, event, git_ref) in [
        ("mbx-store-parity-pr", "pull_request", "refs/pull/1/merge"),
        (
            "mbx-store-parity-queue",
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1-a",
        ),
    ] {
        let other = github_env(name, event, git_ref);
        let mut expected = push.clone();
        assert!(expected.remove("MBX_CACHE_DIR").is_some());
        assert!(expected.contains_key("MBX_SHIMS_DIR"));
        assert!(expected.contains_key("MBX_SHARE_OUT_DIR"));
        assert_eq!(expected, other, "{event} build env");
        assert!(
            !other.contains_key("MBX_CACHE_DIR"),
            "{event} stays untrusted"
        );
    }
}

#[test]
fn an_unusable_shims_path_fails_the_command_on_every_event() {
    for (name, event, git_ref) in [
        ("mbx-store-shims-pr", "pull_request", "refs/pull/1/merge"),
        ("mbx-store-shims-push", "push", "refs/heads/main"),
    ] {
        let run = run_binary(name, event, git_ref, |temp| {
            fs::write(temp.join("velnor-mbx-shims"), "not a directory")
                .unwrap_or_else(|e| unreachable!("{e}"));
        });
        assert!(!run.output.status.success(), "{event}: {:?}", run.output);
        let written = fs::read_to_string(&run.env_file).unwrap_or_default();
        assert!(
            written.is_empty(),
            "{event} left GITHUB_ENV changed: {written}"
        );
    }
}

#[test]
fn an_untrusted_event_creates_or_tightens_the_shims_directory_to_0700() {
    let mode = |temp: &Path| {
        fs::metadata(temp.join("velnor-mbx-shims"))
            .unwrap_or_else(|e| unreachable!("{e}"))
            .permissions()
            .mode()
            & 0o777
    };
    let fresh = run_binary(
        "mbx-store-shims-new",
        "pull_request",
        "refs/pull/1/merge",
        |_| {},
    );
    assert!(fresh.output.status.success(), "{:?}", fresh.output);
    assert_eq!(mode(&fresh.root.join("temp")), 0o700);

    let loose = run_binary(
        "mbx-store-shims-loose",
        "pull_request",
        "refs/pull/1/merge",
        |temp| {
            let dir = temp.join("velnor-mbx-shims");
            fs::create_dir(&dir).unwrap_or_else(|e| unreachable!("{e}"));
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|e| unreachable!("{e}"));
        },
    );
    assert!(loose.output.status.success(), "{:?}", loose.output);
    assert_eq!(mode(&loose.root.join("temp")), 0o700);
}
