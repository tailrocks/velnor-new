// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CiError, append_outputs, break_time_ties, cache_paths, cold_on_failure, current_dir,
    every_root_target, expire_old_bases, now_seconds,
};

pub(super) fn run_restored(args: crate::cli_cache::RestoredArgs) -> Result<String, CiError> {
    let root = current_dir()?;
    let configured = cache_paths::list("VELNOR_CACHE_TARGETS")?.unwrap_or(args.target_dir);
    let targets = if configured.is_empty() {
        every_root_target(&root)?
    } else {
        configured
            .iter()
            .map(|target| cache_paths::validated(&root, target))
            .collect::<Result<Vec<_>, _>>()?
    };
    let expiry = expire_old_bases(&targets, now_seconds())?;
    append_outputs(&format!("expired={}\n", expiry.expired))?;
    let ties = cold_on_failure(&targets, "restored", || break_time_ties(&root))?;
    Ok(expiry.report + &ties)
}

pub(super) fn run_validate_artifact_id() -> Result<String, CiError> {
    crate::preview_helpers::validate_artifact_id(
        &std::env::var(crate::preview_helpers::ARTIFACT_ID_VARIABLE).unwrap_or_default(),
    )?;
    Ok(String::new())
}
