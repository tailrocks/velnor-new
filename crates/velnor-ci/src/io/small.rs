// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CiError, append_outputs, break_time_ties, cold, cold_on_failure, current_dir, expire_old_bases,
    inputs_batch, now_seconds,
};

pub(super) fn run_restored(args: &crate::cli::RestoredArgs) -> Result<String, CiError> {
    let root = current_dir()?;
    let configured = inputs_batch::restored_targets(args)?;
    let (targets, pending) = if configured.is_empty() {
        cold::targets_with_receipts(&root)?
    } else {
        cold::explicit_targets_with_receipts(&root, &configured)?
    };
    let expiry = expire_old_bases(&targets, now_seconds())?;
    append_outputs(&format!("expired={}\n", expiry.expired))?;
    let report = cold_on_failure(&targets, "restored", || break_time_ties(&root))?;
    pending.consume()?;
    Ok(expiry.report + &report)
}

pub(super) fn run_validate_artifact_id() -> Result<String, CiError> {
    crate::preview_helpers::validate_artifact_id(
        &std::env::var(crate::preview_helpers::ARTIFACT_ID_VARIABLE).unwrap_or_default(),
    )?;
    Ok(String::new())
}
