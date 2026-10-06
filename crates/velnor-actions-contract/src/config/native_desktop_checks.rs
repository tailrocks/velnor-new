//! Explicit native validation inputs shared by every desktop operation.

use super::SwiftChecks;
use super::native_desktop::{cargo_name, check, sorted_unique};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Reviewed Swift test-result framework identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SwiftTestFramework {
    /// `XCTest` test-case results.
    #[serde(rename = "xctest")]
    XCTest,
    /// Swift Testing test results.
    #[serde(rename = "swift-testing")]
    SwiftTesting,
}

/// Explicit source and configuration inventory for native checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDesktopChecks {
    /// Sorted unique source directories relative to the native root.
    pub source_dirs: Vec<String>,
    /// Swift formatter configuration relative to the native root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_config: Option<String>,
    /// Swift linter configuration relative to the native root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint_config: Option<String>,
    /// Sorted unique frameworks whose Swift test results must be present.
    #[serde(default)]
    pub swift_test_frameworks: Vec<SwiftTestFramework>,
    /// Sorted unique Cargo packages whose library tests must pass.
    #[serde(default)]
    pub cargo_test_packages: Vec<String>,
    /// Sorted unique Swift executable products exercised in release mode.
    #[serde(default)]
    pub swift_harness_products: Vec<String>,
}

impl NativeDesktopChecks {
    /// Validate source paths, configuration paths, and result expectations.
    /// # Errors
    /// Returns a config diagnostic below the supplied checks key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        SwiftChecks {
            source_dirs: self.source_dirs.clone(),
            format_config: self.format_config.clone(),
            lint_config: self.lint_config.clone(),
            swift_test_frameworks: self.swift_test_frameworks.clone(),
            swift_harness_products: self.swift_harness_products.clone(),
        }
        .validate(file, key)?;
        check(
            file,
            key,
            "cargo_test_packages",
            sorted_unique(&self.cargo_test_packages)
                && self.cargo_test_packages.iter().all(|name| cargo_name(name)),
        )?;
        Ok(())
    }
}
