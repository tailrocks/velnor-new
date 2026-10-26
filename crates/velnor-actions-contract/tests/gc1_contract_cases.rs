// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checks;
mod fixtures;
mod rejects;

use std::error::Error;

#[test]
fn gc1_contract_preserves_contract() -> Result<(), Box<dyn Error>> {
    checks::check_lock_identity()?;
    checks::check_feature_identity()?;
    checks::check_path_identity()?;
    checks::check_profile_identity()?;
    checks::check_toolchain_identity()?;
    checks::check_native_identity()?;
    checks::check_unknown_counters()?;
    checks::check_publication_decisions();
    Ok(())
}

#[test]
fn gc1_contract_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects::rejects_malformed_coverage()?;
    rejects::rejects_malformed_context()?;
    rejects::rejects_malformed_publication();
    rejects::rejects_malformed_lock()?;
    rejects::rejects_collapsed_counters()?;
    Ok(())
}
