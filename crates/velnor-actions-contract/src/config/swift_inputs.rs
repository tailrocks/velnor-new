//! Closed Swift runtime inputs projected from validated generation-time policy.

use super::native_desktop::{
    cargo_name, check, component, deployment_target, identifier, named_artifact_path, path,
    sorted_unique,
};
use super::{AppleAppProfile, NativeDesktopProfile, NativeDesktopTarget, SwiftTestFramework};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Rust-generated artifacts consumed by native Swift operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwiftFfiArtifacts {
    /// Checkout-relative generated bindings directory.
    pub bindings_path: String,
    /// Checkout-relative generated framework directory.
    pub xcframework_path: String,
    /// Generated framework identity.
    pub framework_name: String,
    /// Generated foreign module identity.
    pub module_name: String,
    /// Rust-generated static library basename.
    pub static_library: String,
}

/// Swift-only validation operands; Rust test scope remains with its producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwiftChecks {
    /// Sorted source directories relative to the native root.
    pub source_dirs: Vec<String>,
    /// Swift formatter configuration relative to the native root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_config: Option<String>,
    /// Swift linter configuration relative to the native root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint_config: Option<String>,
    /// Swift test result framework expectations.
    pub swift_test_frameworks: Vec<SwiftTestFramework>,
    /// Native Swift release-mode executable harness products.
    pub swift_harness_products: Vec<String>,
}

/// Native consumer inputs with no Rust producer execution policy.
///
/// Constructed from an already validated [`NativeDesktopProfile`]. The Rust
/// producer binds its complete request digest separately; this projection
/// never supplies the producer's package, manifest, feature, or profile scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwiftInputs {
    /// Rust producer's generated artifact identities.
    pub ffi: SwiftFfiArtifacts,
    /// Checkout-relative native project root.
    pub native_root: String,
    /// Native application project; absent for Swift-package-only checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apple: Option<AppleAppProfile>,
    /// Native validation configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<SwiftChecks>,
    /// Reviewed native target.
    pub target: NativeDesktopTarget,
    /// Minimum macOS release.
    pub deployment_target: String,
}

impl From<&NativeDesktopProfile> for SwiftInputs {
    fn from(profile: &NativeDesktopProfile) -> Self {
        Self {
            ffi: SwiftFfiArtifacts {
                bindings_path: profile.ffi.bindings_path.clone(),
                xcframework_path: profile.ffi.xcframework_path.clone(),
                framework_name: profile.ffi.framework_name.clone(),
                module_name: profile.ffi.module_name.clone(),
                static_library: profile.ffi.static_library.clone(),
            },
            native_root: profile.native_root.clone(),
            apple: profile.apple.clone(),
            checks: profile.checks.as_ref().map(|checks| SwiftChecks {
                source_dirs: checks.source_dirs.clone(),
                format_config: checks.format_config.clone(),
                lint_config: checks.lint_config.clone(),
                swift_test_frameworks: checks.swift_test_frameworks.clone(),
                swift_harness_products: checks.swift_harness_products.clone(),
            }),
            target: profile.target,
            deployment_target: profile.deployment_target.clone(),
        }
    }
}

impl SwiftInputs {
    /// Validate native consumer operands independently of Rust execution policy.
    /// # Errors
    /// Returns a diagnostic below the supplied profile key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        check(
            file,
            key,
            "native_root",
            self.native_root == "." || path(&self.native_root),
        )?;
        check(
            file,
            key,
            "deployment_target",
            deployment_target(&self.deployment_target),
        )?;
        self.ffi.validate(file, &format!("{key}.ffi"))?;
        if let Some(checks) = &self.checks {
            checks.validate(file, &format!("{key}.checks"))?;
        }
        if let Some(apple) = &self.apple {
            apple.validate(file, &format!("{key}.apple"))?;
        }
        Ok(())
    }
}

impl SwiftFfiArtifacts {
    /// Validate generated artifact paths and native identifiers.
    /// # Errors
    /// Returns a diagnostic below the supplied artifacts key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        check(file, key, "bindings_path", path(&self.bindings_path))?;
        check(
            file,
            key,
            "xcframework_path",
            named_artifact_path(&self.xcframework_path, &self.framework_name, "xcframework"),
        )?;
        check(
            file,
            key,
            "framework_name",
            identifier(&self.framework_name)
                && self
                    .framework_name
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_uppercase()),
        )?;
        check(file, key, "module_name", identifier(&self.module_name))?;
        check(
            file,
            key,
            "static_library",
            component(&self.static_library)
                && self.static_library.starts_with("lib")
                && self
                    .static_library
                    .strip_suffix(".a")
                    .is_some_and(|name| name.len() > 3),
        )?;
        Ok(())
    }
}

impl SwiftChecks {
    /// Validate native check inputs without constructing Rust producer scope.
    /// # Errors
    /// Returns a diagnostic below the supplied checks key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        check(
            file,
            key,
            "source_dirs",
            !self.source_dirs.is_empty()
                && sorted_unique(&self.source_dirs)
                && self.source_dirs.iter().all(|dir| path(dir)),
        )?;
        for (field, config) in [
            ("format_config", &self.format_config),
            ("lint_config", &self.lint_config),
        ] {
            check(file, key, field, config.as_deref().is_none_or(path))?;
        }
        check(
            file,
            key,
            "swift_test_frameworks",
            self.swift_test_frameworks.len() <= 2
                && self
                    .swift_test_frameworks
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]),
        )?;
        check(
            file,
            key,
            "swift_harness_products",
            sorted_unique(&self.swift_harness_products)
                && self
                    .swift_harness_products
                    .iter()
                    .all(|name| cargo_name(name)),
        )?;
        Ok(())
    }
}
