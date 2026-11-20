// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_ci::tools_verify::{Discard, Limits, Verb, Verdict, digest_within};
use velnor_ci::tools_verify_step::run;

use crate::tools_verify::{checked, context, discarded, job, recorded};

#[test]
fn a_huge_sparse_file_is_discarded_without_being_read() {
    let job = job("tools-verify-sparse-huge");
    recorded(&job);
    let sparse =
        fs::File::create(job.home.join(".rustup/sparse")).unwrap_or_else(|e| unreachable!("{e}"));
    sparse
        .set_len(10 << 30)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let started = std::time::Instant::now();
    assert!(matches!(
        checked(&job),
        Verdict::Discarded(Discard::Unreadable(_))
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(discarded(&job));
}

#[test]
fn a_small_sparse_file_is_refused_as_suspicious() {
    let job = job("tools-verify-sparse-small");
    recorded(&job);
    fs::File::create(job.home.join(".rustup/hole"))
        .and_then(|file| file.set_len(1 << 20))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        checked(&job),
        Verdict::Discarded(Discard::Unreadable(_))
    ));
}

#[test]
fn a_tree_over_the_total_budget_is_refused() {
    let job = job("tools-verify-budget");
    let limits = Limits {
        file_bytes: 1 << 20,
        tree_bytes: 20,
    };
    assert!(digest_within(&job.home, &limits).is_err());
    assert!(digest_within(&job.home, &Limits::default()).is_ok());
}

#[test]
fn a_hostile_path_cannot_forge_workflow_commands_in_the_notice() {
    let job = job("tools-verify-inject");
    recorded(&job);
    let hostile = job.home.join(".rustup/a\n::stop-commands::x\r%0A");
    fs::File::create(&hostile)
        .and_then(|file| file.set_len(1 << 20))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let out =
        run(Verb::Check, &context(&job, true, "push")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(out.matches('\n').count(), 1, "{out:?}");
    assert!(!out.contains('\r'));
    assert!(!out.contains("::stop-commands::"), "{out:?}");
    assert!(out.starts_with("::notice::"));
    assert!(out.contains("%0A") && out.contains("%25"), "{out:?}");
}
