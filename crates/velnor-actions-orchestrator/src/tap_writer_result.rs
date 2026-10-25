use std::path::Path;

use velnor_actions_contract::canonical_json_bytes;

use super::{CANDIDATE_LIMIT, PR_RECEIPT_FILE, TapPromotionReceipt, read_regular_limited};
use crate::{OrchestratorError, exclusive_write, internal::internal};

pub(super) fn persist_result(
    root: &Path,
    result: &TapPromotionReceipt,
) -> Result<(), OrchestratorError> {
    let path = root.join(PR_RECEIPT_FILE);
    let bytes = canonical_json_bytes(result).map_err(|_| internal("tap_pr_receipt_encode"))?;
    if path.exists() {
        let existing = read_regular_limited(root, &path, CANDIDATE_LIMIT)?;
        if existing != bytes {
            return Err(internal("tap_pr_receipt_conflicting_retry"));
        }
        return Ok(());
    }
    exclusive_write::write_exclusive(&path, &bytes, "tap_pr_receipt")
}
