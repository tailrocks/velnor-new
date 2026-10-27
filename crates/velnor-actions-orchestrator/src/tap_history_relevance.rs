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
    observed_head_repository: Option<(u64, &str)>,
    expected_repository: (u64, &str),
    expected_branch_prefix: &str,
    expected_receipt_marker: &str,
) -> ReceiptHistoryRelevance {
    if observed_head_repository != Some(expected_repository) {
        return ReceiptHistoryRelevance::Unrelated;
    }
    match (
        branch.starts_with(expected_branch_prefix),
        body.contains(expected_receipt_marker),
    ) {
        (false, _) => ReceiptHistoryRelevance::Unrelated,
        (true, true) => ReceiptHistoryRelevance::Relevant,
        _ => ReceiptHistoryRelevance::Conflict,
    }
}
