use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustInputRule {
    pub globs: Vec<String>,

    pub packages: Vec<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
}

pub(super) fn validate_rust_inputs(
    rules: &[RustInputRule],
    file: &str,
) -> Result<(), ContractError> {
    for rule in rules {
        if rule.globs.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.inputs.globs",
                "empty_globs",
            ));
        }
        for glob in &rule.globs {
            if crate::discover::validate_pattern(glob).is_err() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.inputs.globs",
                    format!("bad_glob:{glob}"),
                ));
            }
        }
        if rule.packages.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.inputs.packages",
                "empty_packages",
            ));
        }
        let mut seen = BTreeSet::new();
        for package in &rule.packages {
            let valid = !package.is_empty()
                && package
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !valid {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.inputs.packages",
                    format!("bad_package:{package}"),
                ));
            }
            if !seen.insert(package.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.inputs.packages",
                    format!("duplicate_package:{package}"),
                ));
            }
        }
    }
    Ok(())
}
