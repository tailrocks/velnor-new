// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ReceiptHistoryRelevance, classify};

#[test]
fn receipt_history_relevance_distinguishes_absence_from_conflict() {
    let current = "a".repeat(64);
    let older = "b".repeat(64);
    let current_prefix = format!("velnor/preview/{current}-");
    let current_branch = format!("{current_prefix}{}", "c".repeat(40));
    let older_branch = format!("velnor/preview/{older}-{}", "d".repeat(40));
    let current_marker = format!("Receipt digest: `b3-{current}`");
    let older_marker = format!("Receipt digest: `b3-{older}`");
    let expected_repository = (42, "owner/tap");
    for (case, branch, body, observed_repository, expected) in [
        (
            "current receipt",
            &current_branch,
            &current_marker,
            Some(expected_repository),
            ReceiptHistoryRelevance::Relevant,
        ),
        (
            "older receipt",
            &older_branch,
            &older_marker,
            Some(expected_repository),
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "current branch only",
            &current_branch,
            &older_marker,
            Some(expected_repository),
            ReceiptHistoryRelevance::Conflict,
        ),
        (
            "current marker only",
            &older_branch,
            &current_marker,
            Some(expected_repository),
            ReceiptHistoryRelevance::Unrelated,
        ),
    ] {
        let observed = classify(
            branch,
            body,
            observed_repository,
            expected_repository,
            &current_prefix,
            &current_marker,
        );
        assert_eq!(observed, expected, "{case}");
    }
}

#[test]
fn receipt_history_ignores_foreign_head_repositories() {
    let current = "a".repeat(64);
    let older = "b".repeat(64);
    let current_prefix = format!("velnor/preview/{current}-");
    let current_branch = format!("{current_prefix}{}", "c".repeat(40));
    let current_marker = format!("Receipt digest: `b3-{current}`");
    let older_marker = format!("Receipt digest: `b3-{older}`");
    let expected_repository = (42, "owner/tap");
    for (case, branch, body, observed_repository, expected) in [
        (
            "fork exact namespace with marker",
            &current_branch,
            &current_marker,
            Some((99, "fork/tap")),
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "fork exact namespace without marker",
            &current_branch,
            &older_marker,
            Some((99, "fork/tap")),
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "same id with wrong name",
            &current_branch,
            &current_marker,
            Some((42, "fork/tap")),
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "same name with wrong id",
            &current_branch,
            &current_marker,
            Some((99, "owner/tap")),
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "missing head repository",
            &current_branch,
            &current_marker,
            None,
            ReceiptHistoryRelevance::Unrelated,
        ),
    ] {
        let observed = classify(
            branch,
            body,
            observed_repository,
            expected_repository,
            &current_prefix,
            &current_marker,
        );
        assert_eq!(observed, expected, "{case}");
    }
}
