// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_family::{CacheFamily, FamilyContractPolicy, family_contract_policy};
use crate::errors::ContractError;

pub const FAMILY_KEYS_ID: &str = "family-keys";

pub const FAMILY_SAVE_OUTPUT: &str = "save";

pub const FAMILY_ADMITTED_OUTPUT: &str = "admitted";

pub const FAMILY_WRITER_ENV: &str = "VELNOR_FAMILY_WRITER";

pub const FAMILY_WRITER_EXPR: &str = "${{ matrix.leg }}";

pub const STACK_FAMILIES: [CacheFamily; 5] = [
    CacheFamily::BunInstall,
    CacheFamily::PlaywrightBrowsers,
    CacheFamily::DbtPackages,
    CacheFamily::PythonVenv,
    CacheFamily::AnsibleCollections,
];

#[must_use]
pub fn is_stack_family(family: CacheFamily) -> bool {
    STACK_FAMILIES.contains(&family)
}

#[must_use]
pub fn family_output_stem(family: CacheFamily) -> String {
    family.spec().layer.replace('-', "_")
}

#[must_use]
pub fn family_for_stem(stem: &str) -> Option<CacheFamily> {
    STACK_FAMILIES
        .into_iter()
        .find(|family| family_output_stem(*family) == stem)
}

#[must_use]
pub fn family_key_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_key}}}}",
        family_output_stem(family)
    )
}

#[must_use]
pub fn family_restore_keys_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_restore_keys}}}}",
        family_output_stem(family)
    )
}

#[must_use]
pub fn family_size_step_id(family: CacheFamily) -> String {
    format!("family-size-{}", family.spec().layer)
}

#[must_use]
pub fn family_save_condition(family: CacheFamily) -> String {
    format!(
        "success() && steps.{FAMILY_KEYS_ID}.outputs.{FAMILY_SAVE_OUTPUT} == 'true' && steps.{}.outputs.{FAMILY_ADMITTED_OUTPUT} == 'true'",
        family_size_step_id(family)
    )
}

/// Validate one stack-family plan against the common contract policy.
///
/// Managed Rust families (sources, complete state) must use the common
/// generation path, never the stack path. Stack families keep their own
/// payloads but share the one decimal ceiling and generation-slot limit.
/// A per-root payload requires a valid locator; a shared payload forbids
/// one. Unknown or mismatched plans stay invalid; they never default to
/// a usable family.
pub fn validate_stack_family_plan(
    family: CacheFamily,
    locator: Option<&str>,
) -> Result<FamilyContractPolicy, ContractError> {
    if !is_stack_family(family) {
        return Err(ContractError::identity(
            "cache_family",
            format!("managed_family_requires_gc1_path:{}", family.spec().layer),
        ));
    }
    let per_root = family.spec().is_per_root();
    let locator_ok = match locator {
        Some(locator) => per_root && !locator.is_empty() && root_locator(locator).is_some(),
        None => !per_root,
    };
    if !locator_ok {
        return Err(ContractError::identity(
            "cache_family.locator",
            format!("locator_mismatch:{}", family.spec().layer),
        ));
    }
    let policy = family_contract_policy(family);
    if policy.gc1.is_some() {
        return Err(ContractError::identity(
            "cache_family",
            format!("stack_family_claims_gc1_identity:{}", family.spec().layer),
        ));
    }
    Ok(policy)
}

#[must_use]
pub fn root_locator(root: &str) -> Option<String> {
    let path = root.trim_matches('/');
    if path.is_empty() || path == "." {
        return Some("root".to_owned());
    }
    let safe = root.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    (!safe).then(|| path.replace('/', "-"))
}
