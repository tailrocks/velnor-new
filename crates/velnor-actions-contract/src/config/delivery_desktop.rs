//! Native macOS desktop delivery policy (`[delivery.desktop]`).

use super::NativeDesktopProfile;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Explicit desktop delivery inputs; shell commands and secret names are fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DesktopDeliveryConfig {
    /// Emit desktop build and delivery workflows.
    pub enabled: bool,
    /// Source GitHub repository, exactly `owner/name`.
    pub repository: String,
    /// Protected environment for signed release jobs.
    pub environment: String,
    /// Repository-relative adapter root; `.` selects the checkout root.
    pub root: String,
    /// Exact numeric Xcode release.
    pub xcode_version: String,
    /// Expected lowercase SHA-256 digest of the signing certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_sha256: Option<String>,
    /// Apple Developer Team ID bound to the signing identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Opt in to signing and notarization for version tags.
    pub sign_tags: bool,
    /// Attest desktop artifacts using the canonical generator action.
    pub attest: bool,
    /// Explicit native build profile; required when delivery is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<NativeDesktopProfile>,
}

impl Default for DesktopDeliveryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            repository: String::new(),
            environment: "release-macos".to_owned(),
            root: ".".to_owned(),
            xcode_version: "26.6".to_owned(),
            certificate_sha256: None,
            team_id: None,
            sign_tags: false,
            attest: true,
            profile: None,
        }
    }
}

impl DesktopDeliveryConfig {
    /// Validate drafted values and require signing identity for signed tags.
    /// # Errors
    /// Returns a diagnostic naming the config file and `delivery.desktop` key.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if (self.enabled || !self.repository.is_empty()) && !repository(&self.repository) {
            return Err(invalid(file, "repository", "unsafe_repository"));
        }
        if !component(&self.environment) {
            return Err(invalid(file, "environment", "unsafe_environment"));
        }
        if self.root != "." && (self.root.len() > 1024 || !self.root.split('/').all(component)) {
            return Err(invalid(file, "root", "unsafe_repository_root"));
        }
        if !numeric_version(&self.xcode_version, 0) {
            return Err(invalid(
                file,
                "xcode_version",
                "requires_exact_numeric_version",
            ));
        }
        if let Some(profile) = &self.profile {
            profile.validate(file, "delivery.desktop.profile")?;
            if self.enabled && profile.apple.is_none() {
                return Err(invalid(file, "profile.apple", "missing_apple_application"));
            }
        } else if self.enabled {
            return Err(invalid(file, "profile", "missing_native_desktop_profile"));
        }
        self.validate_signing(file)
    }

    fn validate_signing(&self, file: &str) -> Result<(), ContractError> {
        let required = self.enabled && self.sign_tags;
        if let Some(digest) = &self.certificate_sha256 {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid(
                    file,
                    "certificate_sha256",
                    "requires_lowercase_sha256",
                ));
            }
        } else if required {
            return Err(invalid(
                file,
                "certificate_sha256",
                "missing_signing_identity",
            ));
        }
        if let Some(team) = &self.team_id {
            if team.len() != 10
                || !team
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                return Err(invalid(file, "team_id", "requires_apple_team_id"));
            }
        } else if required {
            return Err(invalid(file, "team_id", "missing_signing_identity"));
        }
        Ok(())
    }
}

fn invalid(file: &str, field: &str, problem: &str) -> ContractError {
    ContractError::config(file, format!("delivery.desktop.{field}"), problem)
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn repository(value: &str) -> bool {
    let mut parts = value.split('/');
    parts.next().is_some_and(component)
        && parts.next().is_some_and(component)
        && parts.next().is_none()
}

fn numeric_version(value: &str, expected_parts: usize) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    value.len() <= 32
        && if expected_parts == 0 {
            (2..=3).contains(&parts.len())
        } else {
            parts.len() == expected_parts
        }
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::DesktopDeliveryConfig;

    #[test]
    fn disabled_default_and_enabled_profile_requirement() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut config: DesktopDeliveryConfig = serde_json::from_str("{}")?;
        assert_eq!(config, DesktopDeliveryConfig::default());
        config.validate("config.toml")?;
        config.enabled = true;
        config.repository = "example/orbit".to_owned();
        let error = config
            .validate("config.toml")
            .expect_err("missing profile accepted");
        assert!(error.to_string().contains("delivery.desktop.profile"));
        Ok(())
    }

    #[test]
    fn rejects_legacy_recipe_fields_and_unsafe_values() -> Result<(), Box<dyn std::error::Error>> {
        for field in [
            "adapter",
            "rust_version",
            "boltffi_version",
            "app_name",
            "task",
        ] {
            let json = serde_json::json!({field: "override"});
            assert!(serde_json::from_value::<DesktopDeliveryConfig>(json).is_err());
        }
        for (field, value) in [
            ("root", "../app"),
            ("repository", "owner/$(id)"),
            ("environment", "release;id"),
            ("xcode_version", "latest"),
            ("certificate_sha256", "ABCDEF"),
            ("team_id", "bad-team"),
        ] {
            let json = serde_json::json!({field: value});
            let config: DesktopDeliveryConfig = serde_json::from_value(json)?;
            assert!(config.validate("config.toml").is_err());
        }
        Ok(())
    }
}
