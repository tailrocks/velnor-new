// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::PullRequestResponse;
use crate::{OrchestratorError, internal::internal};

#[cfg(test)]
#[path = "tap_writer_history_selection_tests.rs"]
mod tests;

pub(super) fn relevant(
    pull: &PullRequestResponse,
    expected_repository_id: u64,
    expected_repository: &str,
    branch_prefix: &str,
    receipt_marker: &str,
    expected_title: &str,
) -> Result<bool, OrchestratorError> {
    use crate::tap_history_relevance::{ReceiptHistoryRelevance, classify};
    match classify(
        &pull.head.ref_name,
        pull.body.as_deref().unwrap_or_default(),
        pull.head
            .repo
            .as_ref()
            .map(|repo| (repo.id, repo.full_name.as_str())),
        (expected_repository_id, expected_repository),
        branch_prefix,
        receipt_marker,
    ) {
        ReceiptHistoryRelevance::Unrelated => return Ok(false),
        ReceiptHistoryRelevance::Conflict => {
            return Err(internal("tap_historical_receipt_marker_conflict"));
        }
        ReceiptHistoryRelevance::Relevant => {}
    }
    if pull.title != expected_title {
        return Err(internal("tap_historical_receipt_title_conflict"));
    }
    Ok(true)
}

pub(super) fn page_finished(length: usize, page: usize) -> Result<bool, OrchestratorError> {
    if length > super::PAGE_SIZE || page == 0 || page > super::MAX_PAGES {
        return Err(internal("tap_historical_pull_page_invalid"));
    }
    if length < super::PAGE_SIZE {
        return Ok(true);
    }
    if page == super::MAX_PAGES {
        return Err(internal("tap_historical_pull_listing_limit"));
    }
    Ok(false)
}
