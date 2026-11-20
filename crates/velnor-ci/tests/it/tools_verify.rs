// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

use velnor_ci::tools_verify::{Anchor, Discard, Recording, Verb, Verdict, check, record};
use velnor_ci::tools_verify_step::{Context, run};

use crate::io::Repo;

pub(crate) struct Job {
    _repo: Repo,
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) anchor: PathBuf,
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

pub(crate) fn rustc(job: &Job) -> PathBuf {
    job.home
        .join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/rustc")
}

fn cosign(job: &Job) -> PathBuf {
    job.home
        .join(".local/share/mise/installs/cosign/3.0.0/bin/cosign")
}

pub(crate) fn recording<'a>(job: &'a Job, anchor: &'a str, verified: bool) -> Recording<'a> {
    Recording {
        anchor: Anchor(anchor),
        file: &job.anchor,
        trusted: true,
        verified,
    }
}

pub(crate) fn recorded(job: &Job) -> String {
    match record(&job.home, &recording(job, "", false)) {
        Ok(Verdict::Recorded(digest)) => digest,
        other => unreachable!("{other:?}"),
    }
}

pub(crate) fn checked(job: &Job, anchor: &str) -> Verdict {
    check(&job.home, &Anchor(anchor)).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn discarded(job: &Job) -> bool {
    [".local/share/mise", ".rustup"]
        .iter()
        .all(|path| fs::symlink_metadata(job.home.join(path)).is_err())
}

#[test]
fn a_restore_equal_to_the_recorded_anchor_is_kept() {
    let job = job("tools-verify-clean");
    let anchor = recorded(&job);
    assert_eq!(checked(&job, &anchor), Verdict::Verified);
    assert_eq!(
        fs::read_to_string(rustc(&job)).ok().as_deref(),
        Some("rustc binary")
    );
    assert_eq!(
        record(&job.home, &recording(&job, &anchor, true)).unwrap_or_else(|e| unreachable!("{e}")),
        Verdict::Matches(anchor)
    );
}

#[test]
fn a_tampered_tool_binary_discards_the_whole_restore() {
    let job = job("tools-verify-tampered");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        checked(&job, &anchor),
        Verdict::Discarded(Discard::Mismatch)
    );
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
        let anchor = recorded(&job);
        change(&job);
        assert_eq!(
            checked(&job, &anchor),
            Verdict::Discarded(Discard::Mismatch),
            "{name}"
        );
        assert!(discarded(&job), "{name}");
    }
}

#[test]
fn a_missing_or_garbled_anchor_discards_the_restore() {
    for (name, anchor, why) in [
        ("none", "", Discard::NoAnchor),
        ("bad", "not-a-digest", Discard::BadAnchor),
        ("upper", &"A".repeat(64), Discard::BadAnchor),
    ] {
        let job = job(&format!("tools-verify-anchor-{name}"));
        assert_eq!(checked(&job, anchor), Verdict::Discarded(why), "{name}");
        assert!(discarded(&job), "{name}");
    }
}

#[test]
fn a_special_file_in_the_restore_is_discarded_unread() {
    let job = job("tools-verify-fifo");
    let anchor = recorded(&job);
    let fifo = job.home.join(".rustup/fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
    assert!(matches!(
        checked(&job, &anchor),
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
    assert_eq!(checked(&job, ""), Verdict::NothingRestored);
    assert!(fs::metadata(&job.home).is_ok());
}

#[test]
fn record_never_blesses_verified_tools_that_changed_afterwards() {
    let job = job("tools-verify-record-differs");
    let anchor = recorded(&job);
    let file = fs::read(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(cosign(&job), "evil cosign!!").unwrap_or_else(|e| unreachable!("{e}"));
    let verdict =
        record(&job.home, &recording(&job, &anchor, true)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Differs(Discard::Mismatch));
    assert_eq!(fs::read(&job.anchor).ok(), Some(file));
}

#[test]
fn a_read_only_directory_in_the_restore_is_still_discarded() {
    for mode in [0o555, 0o000] {
        let job = job(&format!("tools-verify-readonly-{mode:o}"));
        let anchor = recorded(&job);
        let locked = job.home.join(".rustup/toolchains/locked/deep");
        fs::create_dir_all(&locked).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(locked.join("file"), "x").unwrap_or_else(|e| unreachable!("{e}"));
        for dir in [&locked, locked.parent().unwrap_or(&locked)] {
            fs::set_permissions(dir, fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let verdict = checked(&job, &anchor);
        assert!(matches!(verdict, Verdict::Discarded(_)), "{mode:o}");
        assert!(discarded(&job), "{mode:o}");
    }
}

#[test]
fn a_discard_that_cannot_delete_fails_the_step() {
    let job = job("tools-verify-undeletable");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let share = job.home.join(".local/share");
    fs::set_permissions(&share, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let result = check(&job.home, &Anchor(&anchor));
    fs::set_permissions(&share, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.is_err());
}

#[test]
fn only_a_directory_mode_changing_is_a_mismatch() {
    let job = job("tools-verify-dir-mode");
    let anchor = recorded(&job);
    let dir = job
        .home
        .join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin");
    let mode = fs::metadata(&dir)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    fs::set_permissions(&dir, fs::Permissions::from_mode((mode & 0o7777) ^ 0o010))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        checked(&job, &anchor),
        Verdict::Discarded(Discard::Mismatch)
    );
}

pub(crate) fn context(job: &Job, anchor: &str, event: &str) -> Context {
    Context {
        home: job.home.clone(),
        runner_temp: job.root.clone(),
        github_output: Some(job.root.join("github_output")),
        anchor: anchor.to_owned(),
        anchor_matched: !anchor.is_empty(),
        verified: false,
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
    let first = outputs_after(&job, Verb::Record, &context(&job, "", "push"));
    let anchor = recorded_digest(&first);
    assert_eq!(
        first,
        format!("matches=true\nrecorded=true\ndigest={anchor}\n")
    );
    let mut verified = context(&job, &anchor, "push");
    verified.verified = true;
    assert_eq!(
        outputs_after(&job, Verb::Record, &verified),
        format!("matches=true\nrecorded=false\ndigest={anchor}\n")
    );
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, &anchor, "pull_request")),
        "verified=true\n"
    );
    let mut matched = context(&job, "", "pull_request");
    matched.anchor_matched = true;
    assert_eq!(
        outputs_after(&job, Verb::Read, &matched),
        format!("anchor={anchor}\n")
    );
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        outputs_after(&job, Verb::Record, &verified),
        "matches=false\nrecorded=false\n"
    );
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, &anchor, "push")),
        "verified=false\n"
    );
    assert!(discarded(&job));
    assert_eq!(
        outputs_after(&job, Verb::Check, &context(&job, &anchor, "push")),
        "verified=false\n"
    );
}

pub(crate) fn recorded_digest(outputs: &str) -> String {
    outputs
        .lines()
        .find_map(|line| line.strip_prefix("digest="))
        .unwrap_or_default()
        .to_owned()
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
        let mut context = context(&job, "", event);
        context.git_ref = git_ref.to_owned();
        assert_eq!(
            outputs_after(&job, Verb::Record, &context),
            "matches=false\nrecorded=false\n",
            "{event} {git_ref}"
        );
        assert!(fs::symlink_metadata(&job.anchor).is_err(), "{event}");
    }
}

/// Runs `velnor-ci tools-verify <verb>` like a workflow step: a cleared environment, a trusted
/// default-branch push, and the given extra variables.
pub(crate) fn step_output(job: &Job, verb: &str, vars: &[(&str, &str)]) -> String {
    let output = job.root.join("github-output");
    let event = job.root.join("event.json");
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#).unwrap_or_default();
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
        .env("GITHUB_REF", "refs/heads/main")
        .envs(vars.iter().copied());
    let run = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(run.status.success(), "{run:?}");
    fs::read_to_string(output).unwrap_or_default()
}

#[test]
fn only_the_exact_anchor_digest_lets_the_restored_tools_stay() {
    let job = job("tools-verify-env-probe");
    let anchor = recorded_digest(&step_output(&job, "record", &[]));
    assert_eq!(anchor.len(), 64);
    let cases: [(&str, &str, bool); 4] = [
        ("unset", "", false),
        ("garbled", "not-a-digest", false),
        ("other", &"0".repeat(64), false),
        ("exact", &anchor, true),
    ];
    for (name, value, kept) in cases {
        let job = self::job(&format!("tools-verify-env-{name}"));
        step_output(&job, "record", &[]);
        let vars: Vec<(&str, &str)> = if name == "unset" {
            Vec::new()
        } else {
            vec![("VELNOR_TOOLS_ANCHOR", value)]
        };
        let verified = step_output(&job, "check", &vars);
        assert_eq!(
            verified.contains("verified=true"),
            kept,
            "{name}: {verified}"
        );
        assert_eq!(discarded(&job), !kept, "{name}");
    }
}
