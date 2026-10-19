//! Declared Rust inputs outside package directories (`[[stacks.rust.inputs]]`).
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One declared input rule (`[[stacks.rust.inputs]]`).
///
/// A changed path matching any glob affects every named package (and
/// their dependents), in addition to whatever owns the path by
/// directory. Use it for files a package reads outside its own
/// directory: protos compiled by `build.rs`, `include_str!` data, SQL
/// migrations embedded by a proc macro, runtime test fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustInputRule {
    /// Repository-relative globs (`**`, `*`, `?`, `[...]`, `{a,b}`).
    pub globs: Vec<String>,
    /// Exact Cargo package names consuming the matched paths.
    pub packages: Vec<String>,
    /// The named packages' rules, together, list every path they read
    /// outside their directories.
    ///
    /// Affected selection forces a package whose sources read through a
    /// path it cannot resolve statically (`CARGO_MANIFEST_DIR` with `..`, a
    /// computed include) until one rule naming it sets this: the owner's
    /// statement that its declared inputs are complete. Velnor cannot
    /// verify the statement until in-job read verification exists.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
}

/// Validate declared input rules: nonempty well-formed globs and names.
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
