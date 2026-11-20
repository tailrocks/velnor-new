// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::tools_verify::{Discard, Verdict, check, record};

use crate::io::Repo;

struct Job {
    _repo: Repo,
    home: PathBuf,
    anchor: PathBuf,
}

fn job(name: &str) -> Job {
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
        anchor: root.join("anchor/digest"),
        home,
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

fn recorded(job: &Job) {
    let verdict = record(&job.home, &job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Recorded);
}

fn checked(job: &Job) -> Verdict {
    check(&job.home, &job.anchor, true).unwrap_or_else(|e| unreachable!("{e}"))
}

fn discarded(job: &Job) -> bool {
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
        record(&job.home, &job.anchor).unwrap_or_else(|e| unreachable!("{e}")),
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

#[test]
fn a_planted_file_a_new_mode_or_a_moved_link_is_a_mismatch() {
    let changes: [(&str, fn(&Job)); 3] = [
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
fn nothing_restored_leaves_the_home_alone() {
    let job = job("tools-verify-nothing");
    let verdict = check(&job.home, &job.anchor, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::NothingRestored);
    assert!(fs::metadata(cosign(&job)).is_ok());
}

#[test]
fn record_never_blesses_tools_that_differ_from_the_anchor() {
    let job = job("tools-verify-record-differs");
    recorded(&job);
    let anchor = fs::read(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(cosign(&job), "evil cosign!!").unwrap_or_else(|e| unreachable!("{e}"));
    let verdict = record(&job.home, &job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Differs(Discard::Mismatch));
    assert_eq!(fs::read(&job.anchor).ok(), Some(anchor));
}
