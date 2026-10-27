// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiptHistoryRelevance {
    Unrelated,
    Relevant,
    Conflict,
}

#[cfg(test)]
#[path = "tap_history_relevance_tests.rs"]
mod tests;

pub(crate) fn classify(
    branch: &str,
    body: &str,
    expected_branch_prefix: &str,
    expected_receipt_marker: &str,
) -> ReceiptHistoryRelevance {
    match (
        branch.starts_with(expected_branch_prefix),
        body.contains(expected_receipt_marker),
    ) {
        (false, false) => ReceiptHistoryRelevance::Unrelated,
        (true, true) => ReceiptHistoryRelevance::Relevant,
        _ => ReceiptHistoryRelevance::Conflict,
    }
}
