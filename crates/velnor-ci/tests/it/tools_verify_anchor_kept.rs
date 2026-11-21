// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_ci::tools_verify::{Discard, Verdict, read, record};

use crate::tools_verify::{checked, discarded, job, recorded, recording, rustc};

#[test]
fn deleting_only_the_poisoned_tools_entry_reinstalls_and_saves_under_the_same_anchor() {
    let poisoned = job("anchor-kept-poisoned");
    let anchor = recorded(&poisoned);
    fs::write(rustc(&poisoned), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(checked(&poisoned, &anchor), Verdict::Discarded(_)));
    assert!(discarded(&poisoned));

    assert_eq!(checked(&poisoned, &anchor), Verdict::NothingRestored);
    let reinstalled = job("anchor-kept-reinstalled");
    assert_eq!(
        record(&reinstalled.home, &recording(&reinstalled, &anchor, false))
            .unwrap_or_else(|e| unreachable!("{e}")),
        Verdict::Matches(anchor)
    );
}

#[test]
fn an_anchor_that_no_longer_fits_the_reinstalled_tree_is_replaced_by_the_trusted_record() {
    let stale = job("anchor-kept-stale");
    let reinstalled = job("anchor-kept-stale-reinstall");
    fs::write(rustc(&reinstalled), "another image").unwrap_or_else(|e| unreachable!("{e}"));
    let digest = match record(
        &reinstalled.home,
        &recording(&reinstalled, &recorded(&stale), false),
    ) {
        Ok(Verdict::Recorded(digest)) => digest,
        other => unreachable!("{other:?}"),
    };
    assert_eq!(read(&reinstalled.anchor, true), Verdict::Anchored(digest));
}

#[test]
fn a_malformed_anchor_is_replaced_by_the_trusted_record_that_follows_the_discard() {
    let job = job("anchor-kept-malformed");
    let _ = recorded(&job);
    fs::write(&job.anchor, "velnor-tools-anchor-v1 not-a-digest\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        read(&job.anchor, true),
        Verdict::Unanchored(Discard::BadAnchor)
    );
    let digest = match record(&job.home, &recording(&job, "", false)) {
        Ok(Verdict::Recorded(digest)) => digest,
        other => unreachable!("{other:?}"),
    };
    assert_eq!(read(&job.anchor, true), Verdict::Anchored(digest));
}
