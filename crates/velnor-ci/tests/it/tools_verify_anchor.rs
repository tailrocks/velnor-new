// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use velnor_ci::tools_verify::{Discard, Verdict, read, record};

use crate::tools_verify::{checked, job, recorded, recorded_digest, recording, rustc, step_output};

#[test]
fn a_cold_install_that_differs_from_the_anchor_becomes_the_new_anchor() {
    let job = job("tools-verify-reanchor");
    let old = recorded(&job);
    fs::write(rustc(&job), "a new runner image's rustc").unwrap_or_else(|e| unreachable!("{e}"));
    let verdict =
        record(&job.home, &recording(&job, &old, false)).unwrap_or_else(|e| unreachable!("{e}"));
    let Verdict::Recorded(new) = verdict else {
        unreachable!("{verdict:?}")
    };
    assert_ne!(new, old);
    assert_eq!(read(&job.anchor, true), Verdict::Anchored(new.clone()));
    assert_eq!(checked(&job, &new), Verdict::Verified);
}

#[test]
fn a_cold_install_equal_to_the_anchor_keeps_it_and_saves_nothing_new() {
    let job = job("tools-verify-same-anchor");
    let anchor = recorded(&job);
    let file = fs::read(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&job.anchor, "velnor-tools-anchor-v1 changed\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let verdict =
        record(&job.home, &recording(&job, &anchor, false)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verdict, Verdict::Matches(anchor));
    assert_ne!(fs::read(&job.anchor).ok(), Some(file));
}

#[test]
fn a_new_anchor_replaces_whatever_the_anchor_directory_held() {
    let job = job("tools-verify-stale-anchor-dir");
    recorded(&job);
    let dir = job.root.join("velnor-tools-anchor");
    fs::create_dir_all(dir.join("sub/locked")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("sub/locked/file"), "x").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(dir.join("sub/locked"), fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    recorded(&job);
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, ["digest"]);
}

#[test]
fn only_a_restored_entry_lets_the_anchor_file_vouch() {
    let job = job("tools-verify-read");
    let anchor = recorded(&job);
    assert_eq!(
        read(&job.anchor, false),
        Verdict::Unanchored(Discard::NoAnchor)
    );
    assert_eq!(read(&job.anchor, true), Verdict::Anchored(anchor));
    fs::write(&job.anchor, "velnor-tools-anchor-v1 not-a-digest\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        read(&job.anchor, true),
        Verdict::Unanchored(Discard::BadAnchor)
    );
    fs::remove_file(&job.anchor).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        read(&job.anchor, true),
        Verdict::Unanchored(Discard::NoAnchor)
    );
}

#[test]
fn the_read_step_takes_the_anchor_file_only_for_a_restored_entry() {
    let cases = [
        ("matched-unset", None, false),
        ("matched-empty", Some(""), false),
        ("matched-key", Some("velnor-tools-anchor-key"), true),
    ];
    for (name, matched, found) in cases {
        let job = job(name);
        let digest = recorded_digest(&step_output(&job, "record", &[]));
        let vars: Vec<(&str, &str)> = matched
            .map(|key| ("VELNOR_ANCHOR_MATCHED", key))
            .into_iter()
            .collect();
        let read = step_output(&job, "read", &vars);
        let expected = if found {
            format!("anchor={digest}\n")
        } else {
            "anchor=\n".to_owned()
        };
        assert_eq!(read, expected, "{name}");
    }
}
