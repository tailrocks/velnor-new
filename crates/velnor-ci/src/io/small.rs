// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;

use super::{
    CiError, append_outputs, break_time_ties_with, cold, cold_on_failure, current_dir,
    expire_old_bases, inputs, inputs_batch, now_seconds, restore_record,
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
    let report = cold_on_failure(&targets, "restored", || {
        let moved = RefCell::new(restore_record::Restored::default());
        let report = break_time_ties_with(&root, &|path, time| {
            inputs::set_modified_ns(path, time)?;
            moved.borrow_mut().paths.push(path.to_path_buf());
            Ok(())
        })?;
        restore_record::record(&root, &moved.into_inner())?;
        Ok(report)
    })?;
    pending.consume()?;
    Ok(expiry.report + &report)
}

pub(super) fn run_validate_artifact_id() -> Result<String, CiError> {
    crate::preview_helpers::validate_artifact_id(
        &std::env::var(crate::preview_helpers::ARTIFACT_ID_VARIABLE).unwrap_or_default(),
    )?;
    Ok(String::new())
}
