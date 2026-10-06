//! Exact registry ownership policy validation.

use crate::errors::ContractError;
use std::collections::BTreeMap;

pub(super) fn validate_expected_owners(
    file: &str,
    policy: &BTreeMap<String, Vec<String>>,
    enabled: bool,
) -> Result<(), ContractError> {
    let root = "stacks.rust.release.expected_owners";
    if enabled && policy.is_empty() {
        return Err(ContractError::config(file, root, "missing_owner_policy"));
    }
    for (package, owners) in policy {
        if !is_package_name(package) {
            return Err(ContractError::config(file, root, "unsafe_package"));
        }
        let key = format!("{root}.{package}");
        if owners.is_empty() {
            return Err(ContractError::config(file, key, "empty_owner_set"));
        }
        if owners.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::config(
                file,
                key,
                "owners_must_be_sorted_unique",
            ));
        }
        if owners.iter().any(|owner| !qualified_owner(owner)) {
            return Err(ContractError::config(file, key, "invalid_owner_identity"));
        }
    }
    Ok(())
}

fn qualified_owner(owner: &str) -> bool {
    let Some((kind, id)) = owner.split_once(':') else {
        return false;
    };
    matches!(kind, "user" | "team")
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|id| id > 0)
}

/// Cargo package-name shape: start letter/`_`, rest alnum/`-`/`_`.
pub(super) fn is_package_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
