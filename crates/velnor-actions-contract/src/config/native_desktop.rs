//! Declarative native desktop identities; execution belongs to the generator.

use super::NativeDesktopChecks;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Supported native Apple target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeDesktopTarget {
    /// Native Apple Silicon (`aarch64-apple-darwin`, architecture `arm64`).
    #[default]
    AppleArm64,
}

/// Cargo producer and generated foreign-language artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustFfiProfile {
    /// Checkout-relative Cargo producer manifest.
    pub manifest_path: String,
    /// Cargo package selected for the static library build.
    pub package: String,
    /// Cargo build profile.
    pub profile: String,
    /// Sorted unique feature allowlist; default Cargo features remain enabled.
    #[serde(default)]
    pub features: Vec<String>,
    /// Generated framework identity.
    pub framework_name: String,
    /// Generated foreign module identifier.
    pub module_name: String,
    /// Rust static library basename (`lib*.a`).
    pub static_library: String,
    /// Checkout-relative generated bindings directory.
    pub bindings_path: String,
    /// Checkout-relative generated `.xcframework` directory.
    pub xcframework_path: String,
}

/// Xcode application project and bundle identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleAppProfile {
    /// Project specification relative to the native root (`.yml` or `.yaml`).
    pub project_spec: String,
    /// Generated Xcode project relative to the native root.
    pub project_path: String,
    /// Xcode application and test scheme.
    pub scheme: String,
    /// Application executable basename.
    pub app_name: String,
    /// Reverse-DNS bundle identifier.
    pub bundle_identifier: String,
    /// Human-readable bundle name, a printable single line.
    pub bundle_name: String,
    /// Checkout-relative generated application bundle.
    pub app_path: String,
    /// Checkout-relative isolated Xcode derived-data directory.
    pub derived_data_path: String,
    /// Stable archive basename prefix.
    pub archive_name_prefix: String,
    /// Sorted unique paths relative to the app bundle that must exist.
    #[serde(default)]
    pub required_resources: Vec<String>,
    /// Native application unit-test target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_target: Option<String>,
    /// Native application UI-test target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_test_target: Option<String>,
    /// Unit-test sources relative to the native root.
    #[serde(default)]
    pub test_sources: Vec<String>,
    /// UI-test sources relative to the native root.
    #[serde(default)]
    pub ui_test_sources: Vec<String>,
    /// Expected `LSUIElement` value in the generated bundle.
    #[serde(default)]
    pub bundle_lsui_element: bool,
}

/// Shared native Rust/Apple profile for validation and delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDesktopProfile {
    /// Rust static-library producer and generated FFI artifacts.
    pub ffi: RustFfiProfile,
    /// Checkout-relative native package/project root; `.` is allowed.
    pub native_root: String,
    /// Xcode application; absent for Swift-package-only validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apple: Option<AppleAppProfile>,
    /// Native source/check inputs; operations require their explicit inventory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<NativeDesktopChecks>,
    /// Reviewed native target; no cross-compilation override.
    #[serde(default)]
    pub target: NativeDesktopTarget,
    /// Exact minimum macOS release (`major.minor`).
    pub deployment_target: String,
}

impl NativeDesktopProfile {
    /// # Errors
    /// Returns a config diagnostic scoped under the supplied key.
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

impl RustFfiProfile {
    /// # Errors
    /// Returns a key-qualified config diagnostic for invalid values.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        check(
            file,
            key,
            "manifest_path",
            path(&self.manifest_path)
                && self.manifest_path.rsplit('/').next() == Some("Cargo.toml"),
        )?;
        check(file, key, "package", cargo_name(&self.package))?;
        check(file, key, "profile", cargo_name(&self.profile))?;
        check(
            file,
            key,
            "features",
            sorted_unique(&self.features)
                && self.features.iter().all(|feature| cargo_feature(feature)),
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
        check(file, key, "bindings_path", path(&self.bindings_path))?;
        check(
            file,
            key,
            "xcframework_path",
            named_artifact_path(&self.xcframework_path, &self.framework_name, "xcframework"),
        )?;
        Ok(())
    }
}

impl AppleAppProfile {
    /// # Errors
    /// Returns a key-qualified config diagnostic for invalid values.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        check(
            file,
            key,
            "project_spec",
            path(&self.project_spec)
                && (extension(&self.project_spec, "yml") || extension(&self.project_spec, "yaml")),
        )?;
        check(
            file,
            key,
            "project_path",
            path(&self.project_path) && extension(&self.project_path, "xcodeproj"),
        )?;
        for (field, value) in [
            ("scheme", &self.scheme),
            ("app_name", &self.app_name),
            ("archive_name_prefix", &self.archive_name_prefix),
        ] {
            check(file, key, field, component(value))?;
        }
        check(
            file,
            key,
            "bundle_identifier",
            bundle_identifier(&self.bundle_identifier),
        )?;
        check(
            file,
            key,
            "bundle_name",
            !self.bundle_name.is_empty()
                && self.bundle_name.len() <= 255
                && self.bundle_name.trim() == self.bundle_name
                && !self.bundle_name.chars().any(char::is_control)
                && !self.bundle_name.contains("${{"),
        )?;
        check(
            file,
            key,
            "app_path",
            named_artifact_path(&self.app_path, &self.app_name, "app"),
        )?;
        check(
            file,
            key,
            "derived_data_path",
            path(&self.derived_data_path),
        )?;
        check(
            file,
            key,
            "required_resources",
            sorted_unique(&self.required_resources)
                && self
                    .required_resources
                    .iter()
                    .all(|resource| path(resource)),
        )?;
        self.validate_tests(file, key)
    }

    fn validate_tests(&self, file: &str, key: &str) -> Result<(), ContractError> {
        for (field, target, sources) in [
            ("test_target", &self.test_target, &self.test_sources),
            (
                "ui_test_target",
                &self.ui_test_target,
                &self.ui_test_sources,
            ),
        ] {
            check(file, key, field, target.as_deref().is_none_or(component))?;
            check(file, key, field, target.is_some() != sources.is_empty())?;
            let source_field = if field == "test_target" {
                "test_sources"
            } else {
                "ui_test_sources"
            };
            check(
                file,
                key,
                source_field,
                sorted_unique(sources) && sources.iter().all(|source| path(source)),
            )?;
        }
        Ok(())
    }
}

pub(super) fn check(file: &str, key: &str, field: &str, valid: bool) -> Result<(), ContractError> {
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(
            file,
            format!("{key}.{field}"),
            "unsafe_native_desktop_value",
        ))
    }
}

pub(super) fn deployment_target(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    value.len() <= 16
        && parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(super) fn path(value: &str) -> bool {
    value.len() <= 1024 && value.split('/').all(component)
}

pub(super) fn named_artifact_path(value: &str, name: &str, suffix: &str) -> bool {
    path(value)
        && value
            .rsplit('/')
            .next()
            .is_some_and(|leaf| leaf == format!("{name}.{suffix}"))
}

pub(super) fn extension(value: &str, expected: &str) -> bool {
    value
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, found)| !stem.is_empty() && found == expected)
}

fn bundle_identifier(value: &str) -> bool {
    let segments: Vec<_> = value.split('.').collect();
    value.len() <= 255
        && segments.len() >= 2
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !segment.ends_with('-')
        })
}

pub(super) fn cargo_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn cargo_feature(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(super) fn sorted_unique(values: &[String]) -> bool {
    values.len() <= 128 && values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
#[path = "native_desktop_tests.rs"]
mod tests;
