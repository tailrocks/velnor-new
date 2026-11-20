// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::tools_verify::{Anchor, Discard, Verb, Verdict, check, record};
use velnor_ci::tools_verify_step::{Context, run};

use crate::io::Repo;

pub(crate) struct Job {
    _repo: Repo,
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    anchor: PathBuf,
}

pub(crate) fn job(name: &str) -> Job {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let home = root.join("home");
    let mise = home.join(".local/share/mise/installs/cosign/3.0.0/bin");
    fs::create_dir_all(&mise).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(mise.join("cosign"), "cosign binary").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(mise.join("cosign"), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink(
        "3.0.0",
        home.join(".local/share/mise/installs/cosign/latest"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let toolchain = home.join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin");
    fs::create_dir_all(&toolchain).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(toolchain.join("rustc"), "rustc binary").unwrap_or_else(|e| unreachable!("{e}"));
    Job {
        _repo: repo,
        anchor: root.join("velnor-tools-anchor/digest"),
        home,
        root,
    }
}

fn rustc(job: &Job) -> PathBuf {
    job.home
        .join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/rustc")
}

fn cosign(job: &Job) -> PathBuf {
    job.home
        .join(".local/share/mise/installs/cosign/3.0.0/bin/cosign")
}

fn restored(job: &Job) -> Anchor<'_> {
    Anchor {
        file: &job.anchor,
        restored: true,
    }
}

pub(crate) fn recorded(job: &Job) {
    let verdict = record(&job.home, &restored(job), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Recorded);
}

pub(crate) fn checked(job: &Job) -> Verdict {
    check(&job.home, &restored(job)).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn discarded(job: &Job) -> bool {
    [".local/share/mise", ".rustup"]
        .iter()
        .all(|path| fs::symlink_metadata(job.home.join(path)).is_err())
}

#[test]
fn a_restore_equal_to_the_recorded_anchor_is_kept() {
    let job = job("tools-verify-clean");
    recorded(&job);
    assert_eq!(checked(&job), Verdict::Verified);
    assert_eq!(
        fs::read_to_string(rustc(&job)).ok().as_deref(),
        Some("rustc binary")
    );
    assert_eq!(
        record(&job.home, &restored(&job), true).unwrap_or_else(|e| unreachable!("{e}")),
        Verdict::Matches
    );
}

#[test]
fn a_tampered_tool_binary_discards_the_whole_restore() {
    let job = job("tools-verify-tampered");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(checked(&job), Verdict::Discarded(Discard::Mismatch));
    assert!(discarded(&job), "both restored trees must be deleted");
}

type Change = (&'static str, fn(&Job));

#[test]
fn a_planted_file_a_new_mode_or_a_moved_link_is_a_mismatch() {
    let changes: [Change; 3] = [
        ("planted", |job| {
            fs::write(
                job.home
                    .join(".local/share/mise/installs/cosign/3.0.0/bin/cargo"),
                "x",
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }),
        ("mode", |job| {
            fs::set_permissions(rustc(job), fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }),
        ("link", |job| {
            let link = job.home.join(".local/share/mise/installs/cosign/latest");
            fs::remove_file(&link).unwrap_or_else(|e| unreachable!("{e}"));
            symlink("/tmp/evil", &link).unwrap_or_else(|e| unreachable!("{e}"));
        }),
    ];
    for (name, change) in changes {
        let job = job(&format!("tools-verify-{name}"));
        recorded(&job);
        change(&job);
        assert_eq!(
            checked(&job),
            Verdict::Discarded(Discard::Mismatch),
            "{name}"
        );
        assert!(discarded(&job), "{name}");
    }
}

#[test]
fn a_missing_or_garbled_anchor_discards_the_restore() {
    let job = job("tools-verify-no-anchor");
    assert_eq!(checked(&job), Verdict::Discarded(Discard::NoAnchor));
    assert!(discarded(&job));
    let job = self::job("tools-verify-bad-anchor");
    fs::create_dir_all(job.anchor.parent().unwrap_or(Path::new("/")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&job.anchor, "velnor-tools-anchor-v1 not-a-digest\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(checked(&job), Verdict::Discarded(Discard::BadAnchor));
    assert!(discarded(&job));
}

#[test]
fn a_special_file_in_the_restore_is_discarded_unread() {
    let job = job("tools-verify-fifo");
    recorded(&job);
    let fifo = job.home.join(".rustup/fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
    assert!(matches!(
        checked(&job),
        Verdict::Discarded(Discard::Unreadable(_))
    ));
    assert!(discarded(&job));
}

#[test]
fn a_home_without_tool_directories_has_nothing_to_verify() {
    let job = job("tools-verify-nothing");
    for path in [".local/share/mise", ".rustup"] {
        fs::remove_dir_all(job.home.join(path)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let verdict = check(&job.home, &restored(&job)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::NothingRestored);
    assert!(fs::metadata(&job.home).is_ok());
}

#[test]
fn record_never_blesses_tools_that_differ_from_the_anchor() {
    let job = job("tools-verify-record-differs");
    recorded(&job);
    let anchor = fs::read(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(cosign(&job), "evil cosign!!").unwrap_or_else(|e| unreachable!("{e}"));
    let verdict = record(&job.home, &restored(&job), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Differs(Discard::Mismatch));
    assert_eq!(fs::read(&job.anchor).ok(), Some(anchor));
}

fn missed(job: &Job) -> Anchor<'_> {
    Anchor {
        file: &job.anchor,
        restored: false,
    }
}

#[test]
fn a_leftover_anchor_is_not_trusted_when_the_anchor_restore_missed() {
    let job = job("tools-verify-stale-anchor");
    recorded(&job);
    let verdict = check(&job.home, &missed(&job)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Discarded(Discard::NoAnchor));
    assert!(discarded(&job));
}

#[test]
fn a_leftover_anchor_never_lets_record_call_the_tools_matching() {
    let job = job("tools-verify-stale-record");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stale = fs::read(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    let verdict = record(&job.home, &missed(&job), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Recorded);
    assert_ne!(fs::read(&job.anchor).ok(), Some(stale));
}

#[test]
fn reset_deletes_the_anchor_directory_with_whatever_it_holds() {
    let job = job("tools-verify-reset");
    recorded(&job);
    let dir = job.root.join("velnor-tools-anchor");
    fs::create_dir_all(dir.join("sub/locked")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("sub/locked/file"), "x").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(dir.join("sub/locked"), fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let context = context(&job, false, "push");
    run(Verb::Reset, &context).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(fs::symlink_metadata(&dir).is_err());
    run(Verb::Reset, &context).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_read_only_directory_in_the_restore_is_still_discarded() {
    for mode in [0o555, 0o000] {
        let job = job(&format!("tools-verify-readonly-{mode:o}"));
        recorded(&job);
        let locked = job.home.join(".rustup/toolchains/locked/deep");
        fs::create_dir_all(&locked).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(locked.join("file"), "x").unwrap_or_else(|e| unreachable!("{e}"));
        for dir in [&locked, locked.parent().unwrap_or(&locked)] {
            fs::set_permissions(dir, fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let verdict = checked(&job);
        assert!(matches!(verdict, Verdict::Discarded(_)), "{mode:o}");
        assert!(discarded(&job), "{mode:o}");
    }
}

#[test]
fn a_discard_that_cannot_delete_fails_the_step() {
    let job = job("tools-verify-undeletable");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let share = job.home.join(".local/share");
    fs::set_permissions(&share, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let result = check(&job.home, &restored(&job));
    fs::set_permissions(&share, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.is_err());
}

#[test]
fn only_a_directory_mode_changing_is_a_mismatch() {
    let job = job("tools-verify-dir-mode");
    recorded(&job);
    let dir = job
        .home
        .join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin");
    let mode = fs::metadata(&dir)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    fs::set_permissions(&dir, fs::Permissions::from_mode((mode & 0o7777) ^ 0o010))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(checked(&job), Verdict::Discarded(Discard::Mismatch));
}

pub(crate) fn context(job: &Job, anchor_restored: bool, event: &str) -> Context {
    Context {
        home: job.home.clone(),
        runner_temp: job.root.clone(),
        github_output: Some(job.root.join("github_output")),
        anchor_restored,
        event_name: event.to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        default_branch: Some("main".to_owned()),
    }
}

fn outputs_after(job: &Job, verb: Verb, context: &Context) -> String {
    let _ = fs::remove_file(job.root.join("github_output"));
    run(verb, context).unwrap_or_else(|e| unreachable!("{e}"));
    fs::read_to_string(job.root.join("github_output")).unwrap_or_default()
}

#[test]
fn the_step_writes_the_outputs_that_gate_every_save() {
    let job = job("tools-verify-outputs");
    assert_eq!(
        outputs_after(&job, Verb::Record, &context(&job, false, "push")),
        "matches=true\nrecorded=true\n"
    );
    assert_eq!(
        outputs_after(&job, Verb::Record, &context(&job, true, "push")),
        "matches=true\nrecorded=false\n"
    );
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, true, "pull_request")),
        "verified=true\n"
    );
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        outputs_after(&job, Verb::Record, &context(&job, true, "push")),
        "matches=false\nrecorded=false\n"
    );
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, true, "push")),
        "verified=false\n"
    );
    assert!(discarded(&job));
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, true, "push")),
        "verified=false\n"
    );
}

#[test]
fn record_refuses_an_untrusted_event_and_writes_no_anchor() {
    for (event, git_ref) in [
        ("pull_request", "refs/heads/main"),
        ("merge_group", "refs/heads/main"),
        ("workflow_run", "refs/heads/main"),
        ("push", "refs/heads/feature"),
        ("push", "refs/pull/7/merge"),
    ] {
        let job = job("tools-verify-untrusted-record");
        let mut context = context(&job, false, event);
        context.git_ref = git_ref.to_owned();
        assert_eq!(
            outputs_after(&job, Verb::Record, &context),
            "matches=false\nrecorded=false\n",
            "{event} {git_ref}"
        );
        assert!(fs::symlink_metadata(&job.anchor).is_err(), "{event}");
    }
}

/// Runs the built `velnor-ci tools-verify <verb>` the way a workflow step does: a cleared
/// environment, a trusted default-branch push, and `VELNOR_ANCHOR_HIT` set to `hit` (unset when
/// `None`). Returns what the step wrote to `GITHUB_OUTPUT`.
fn step_output(job: &Job, verb: &str, hit: Option<&str>) -> String {
    let output = job.root.join("github-output");
    let event = job.root.join("event.json");
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ = fs::remove_file(&output);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
        .args(["tools-verify", verb])
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", &job.root)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &event)
        .env("GITHUB_REF", "refs/heads/main");
    if let Some(hit) = hit {
        command.env("VELNOR_ANCHOR_HIT", hit);
    }
    let run = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    fs::read_to_string(output).unwrap_or_default()
}

#[test]
fn only_the_exact_hit_true_lets_the_restored_anchor_vouch_for_the_tools() {
    let cases = [
        ("hit-unset", None),
        ("hit-empty", Some("")),
        ("hit-false", Some("false")),
        ("hit-true", Some("true")),
    ];
    for (name, hit) in cases {
        let job = job(name);
        assert!(step_output(&job, "record", None).contains("recorded=true"), "{name}");
        let verified = step_output(&job, "check", hit);
        if hit == Some("true") {
            assert!(verified.contains("verified=true"), "{name}: {verified}");
            assert!(!discarded(&job), "{name}");
        } else {
            assert!(verified.contains("verified=false"), "{name}: {verified}");
            assert!(discarded(&job), "{name}");
        }
    }
}
