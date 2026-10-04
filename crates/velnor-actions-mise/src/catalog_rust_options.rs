//! Typed Rust backend options supported by pinned Mise 2026.10.0.
//!
//! `args/tool_arg.rs` accepts backend options before `@`; the Rust backend
//! consumes `profile`, `components`, and `targets` for install and checks
//! requested state during availability verification. Repository strings
//! never become arbitrary backend options.

use crate::{MiseError, catalog::validate_exact_version, steps::validate_step_token};

/// Minimal Rust profile with an explicit, canonical installation closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustInstallOptions {
    components: Vec<String>,
    targets: Vec<String>,
    mr_boxington: bool,
}

impl RustInstallOptions {
    /// Default generator obligations require Clippy and rustfmt.
    #[must_use]
    pub fn required() -> Self {
        Self {
            components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
            targets: Vec::new(),
            mr_boxington: false,
        }
    }

    /// Closed desktop obligations need the host target and nested MBX routing.
    #[must_use]
    pub fn desktop() -> Self {
        let mut options = Self::desktop_source();
        options.mr_boxington = true;
        options
    }

    /// Protected native delivery uses the same compiler without an MBX wrapper.
    #[must_use]
    pub fn desktop_source() -> Self {
        let mut options = Self::required();
        options.targets.push("aarch64-apple-darwin".to_owned());
        options
    }

    /// Add required source/components and cross targets without option injection.
    ///
    /// # Errors
    ///
    /// Rejects empty or shell/backend-hostile component and target tokens.
    pub fn with_requirements(components: &[String], targets: &[String]) -> Result<Self, MiseError> {
        let mut options = Self::required();
        for value in components {
            validate_step_token("rust_component", value)?;
            options.components.push(value.clone());
        }
        for value in targets {
            validate_step_token("rust_target", value)?;
            options.targets.push(value.clone());
        }
        options.components.sort();
        options.components.dedup();
        options.targets.sort();
        options.targets.dedup();
        Ok(options)
    }

    /// Exact backend CLI selector; only typed supported option names are emitted.
    ///
    /// # Errors
    ///
    /// Rejects a version which is not an exact numeric catalog pin.
    pub fn tool_spec(&self, version: &str) -> Result<String, MiseError> {
        validate_exact_version("rust", version)?;
        Ok(self.pinned_spec(version))
    }

    /// Format an already validated catalog pin.
    pub(crate) fn pinned_spec(&self, version: &str) -> String {
        let mut options = format!("profile=minimal,components={}", self.components.join(","));
        if !self.targets.is_empty() {
            options.push_str(",targets=");
            options.push_str(&self.targets.join(","));
        }
        if self.mr_boxington {
            options.push_str(",mr_boxington=true");
        }
        format!("rust[{options}]@{version}")
    }

    /// Canonical component inventory for tool identity and verification.
    #[must_use]
    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Canonical additional target inventory.
    #[must_use]
    pub fn targets(&self) -> &[String] {
        &self.targets
    }
}

#[cfg(test)]
mod tests {
    use super::RustInstallOptions;

    #[test]
    fn canonical_required_profile_preserves_explicit_extra_obligations() -> Result<(), String> {
        let options = RustInstallOptions::with_requirements(
            &["rust-src".to_owned(), "clippy".to_owned()],
            &[
                "wasm32-unknown-unknown".to_owned(),
                "wasm32-unknown-unknown".to_owned(),
            ],
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            options
                .tool_spec("1.98.1")
                .map_err(|error| error.to_string())?,
            "rust[profile=minimal,components=clippy,rust-src,rustfmt,targets=wasm32-unknown-unknown]@1.98.1"
        );
        Ok(())
    }

    #[test]
    fn arbitrary_backend_options_and_loose_versions_are_rejected() {
        for value in ["", "rustfmt,profile=default", "rust-src]", "$(id)", "a b"] {
            assert!(RustInstallOptions::with_requirements(&[value.to_owned()], &[]).is_err());
            assert!(RustInstallOptions::with_requirements(&[], &[value.to_owned()]).is_err());
        }
        assert!(RustInstallOptions::required().tool_spec("stable").is_err());
    }
}
