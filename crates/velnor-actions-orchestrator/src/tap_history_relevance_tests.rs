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
    for (case, branch, body, expected) in [
        (
            "current receipt",
            &current_branch,
            &current_marker,
            ReceiptHistoryRelevance::Relevant,
        ),
        (
            "older receipt",
            &older_branch,
            &older_marker,
            ReceiptHistoryRelevance::Unrelated,
        ),
        (
            "current branch only",
            &current_branch,
            &older_marker,
            ReceiptHistoryRelevance::Conflict,
        ),
        (
            "current marker only",
            &older_branch,
            &current_marker,
            ReceiptHistoryRelevance::Conflict,
        ),
    ] {
        assert_eq!(
            classify(branch, body, &current_prefix, &current_marker),
            expected,
            "{case}"
        );
    }
}
