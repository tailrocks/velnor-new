//! Release-manifest and `generator.lock` schemas plus per-target records.
//!
//! Serde shapes only; file IO and TOML parsing live outside this crate.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

pub use crate::candidate_manifest::CandidateArtifactManifest;
pub use crate::candidate_manifest::require_release_version;

/// Versioned release manifest: one immutable asset record per target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    /// Manifest schema version; must be 1.
    pub schema: u32,
    /// Exact release version (`X.Y.Z`).
    pub version: String,
    /// Canonical repository identity.
    pub repository: String,
    /// Per-target asset records.
    pub targets: Vec<TargetRecord>,
}

/// One immutable per-target release asset record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRecord {
    /// Target triple.
    pub target: String,
    /// Immutable asset URL.
    pub artifact: String,
    /// SHA-256 of the asset (64 lowercase hex).
    pub sha256: String,
}

/// Velnor-repository-only bootstrap lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorLock {
    /// Lock schema version; must be 1.
    pub schema: u32,
    /// Locked generator binaries.
    pub generator: LockedGenerator,
    /// Pinned Mise bootstrap record.
    pub mise_bootstrap: MiseBootstrap,
    /// Reviewed action pins.
    pub actions: Vec<ActionPin>,
}

/// Locked generator binary section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedGenerator {
    /// Binary name (`velnor-actions`).
    pub binary: String,
    /// Exact locked version.
    pub version: String,
    /// One record per supported target.
    pub binaries: Vec<GeneratorBinary>,
}

/// One per-target locked generator binary record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorBinary {
    /// Target triple.
    pub target: String,
    /// Immutable asset URL.
    pub artifact: String,
    /// SHA-256 of the asset (64 lowercase hex).
    pub sha256: String,
}

/// Pinned Mise bootstrap record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseBootstrap {
    /// Exact Mise version.
    pub version: String,
    /// Immutable asset URL.
    pub artifact: String,
    /// SHA-256 of the asset (64 lowercase hex).
    pub sha256: String,
}

/// One reviewed action pin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPin {
    /// Action name (`owner/repo`).
    pub name: String,
    /// Review label (`vX.Y.Z`).
    pub version: String,
    /// Full 40-char commit SHA (lowercase hex).
    pub sha: String,
    /// Review date (`YYYY-MM-DD`).
    pub reviewed: String,
}

impl ReleaseManifest {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;

    /// Parse canonical release-manifest JSON, rejecting malformed input.
    ///
    /// Duplicate keys are rejected (cache §1); unknown keys fail with
    /// `unknown_config_field` (arch §3).
    /// # Errors
    pub fn parse_json(text: &str, file: &str) -> Result<Self, ContractError> {
        let value = crate::strict_json::parse_strict_json(text).map_err(|err| {
            ContractError::config(file, "document", format!("malformed_json:{err}"))
        })?;
        serde_json::from_value(value).map_err(|err| map_manifest_error(file, &err))
    }

    /// Find the asset record for a target triple.
    #[must_use]
    pub fn record_for_target(&self, target: &str) -> Option<&TargetRecord> {
        self.targets.iter().find(|record| record.target == target)
    }

    /// Validate schema, version, repository, and every target record.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        check_semver(&self.version, file, "version")?;
        if self.repository.trim().is_empty() {
            return Err(ContractError::config(
                file,
                "repository",
                "empty_repository",
            ));
        }
        if self.targets.is_empty() {
            return Err(ContractError::config(file, "targets", "empty_targets"));
        }
        let mut seen = BTreeSet::new();
        for record in &self.targets {
            check_target(&record.target, file, "targets.target")?;
            check_immutable_url(&record.artifact, file, "targets.artifact")?;
            check_sha256(&record.sha256, file, "targets.sha256")?;
            if !seen.insert(record.target.as_str()) {
                return Err(ContractError::config(
                    file,
                    "targets",
                    format!("duplicate_target:{}", record.target),
                ));
            }
        }
        Ok(())
    }
}

impl GeneratorLock {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;

    /// Require exactly one binary record per supported target, no extras.
    /// # Errors
    pub fn check_supported_targets(&self, file: &str) -> Result<(), ContractError> {
        for target in crate::targets::SUPPORTED_TARGETS {
            if self.binary_for_target(target).is_none() {
                return Err(ContractError::config(
                    file,
                    "generator.binaries",
                    format!("missing_target:{target}"),
                ));
            }
        }
        for record in &self.generator.binaries {
            if !crate::targets::is_supported_target(&record.target) {
                return Err(ContractError::config(
                    file,
                    "generator.binaries",
                    format!("unsupported_target:{}", record.target),
                ));
            }
        }
        Ok(())
    }

    /// Find the binary record for a target triple.
    #[must_use]
    pub fn binary_for_target(&self, target: &str) -> Option<&GeneratorBinary> {
        self.generator
            .binaries
            .iter()
            .find(|record| record.target == target)
    }

    /// Validate lock schema, binaries, bootstrap, and action pins.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        if self.generator.binary.trim().is_empty() {
            return Err(ContractError::config(
                file,
                "generator.binary",
                "empty_binary",
            ));
        }
        check_semver(&self.generator.version, file, "generator.version")?;
        if self.generator.binaries.is_empty() {
            return Err(ContractError::config(
                file,
                "generator.binaries",
                "empty_binaries",
            ));
        }
        let mut seen = BTreeSet::new();
        for record in &self.generator.binaries {
            check_target(&record.target, file, "generator.binaries.target")?;
            check_immutable_url(&record.artifact, file, "generator.binaries.artifact")?;
            check_sha256(&record.sha256, file, "generator.binaries.sha256")?;
            if !seen.insert(record.target.as_str()) {
                return Err(ContractError::config(
                    file,
                    "generator.binaries",
                    format!("duplicate_target:{}", record.target),
                ));
            }
        }
        check_semver(&self.mise_bootstrap.version, file, "mise-bootstrap.version")?;
        check_immutable_url(
            &self.mise_bootstrap.artifact,
            file,
            "mise-bootstrap.artifact",
        )?;
        check_sha256(&self.mise_bootstrap.sha256, file, "mise-bootstrap.sha256")?;
        let mut actions = BTreeSet::new();
        for pin in &self.actions {
            if pin.name.trim().is_empty() || !pin.name.contains('/') {
                return Err(ContractError::config(
                    file,
                    "actions.name",
                    "malformed_action_name",
                ));
            }
            if pin.version.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "actions.version",
                    "empty_version",
                ));
            }
            if pin.sha.len() != 40 || !is_lower_hex(&pin.sha) {
                return Err(ContractError::config(file, "actions.sha", "malformed_sha"));
            }
            if !is_review_date(&pin.reviewed) {
                return Err(ContractError::config(
                    file,
                    "actions.reviewed",
                    "malformed_date",
                ));
            }
            if !actions.insert(pin.name.as_str()) {
                return Err(ContractError::config(
                    file,
                    "actions",
                    format!("duplicate_action:{}", pin.name),
                ));
            }
        }
        Ok(())
    }
}

/// Map a manifest decode error: unknown keys become `unknown_config_field`.
fn map_manifest_error(file: &str, err: &serde_json::Error) -> ContractError {
    let message = err.to_string();
    if let Some(rest) = message.strip_prefix("unknown field `")
        && let Some((field, _)) = rest.split_once('`')
    {
        return ContractError::unknown_config_field(file, field);
    }
    ContractError::config(file, "document", format!("malformed_json:{message}"))
}

/// Check a document schema version.
pub(crate) fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}

/// Check exact `X.Y.Z` numeric semver.
fn check_semver(version: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = version.split('.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_semver"))
    }
}

/// Check target-triple shape.
pub(crate) fn check_target(target: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let valid = !target.is_empty()
        && target.contains('-')
        && target.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_target"))
    }
}

/// Check an immutable `https://` asset URL (no floating segments).
fn check_immutable_url(url: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(ContractError::config(file, key, "mutable_or_malformed_url"));
    };
    let valid = !rest.is_empty()
        && !url.contains(' ')
        && !rest.split('/').any(|seg| seg.is_empty() || seg == "latest");
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "mutable_or_malformed_url"))
    }
}

/// Check a SHA-256 hex digest.
pub(crate) fn check_sha256(sha: &str, file: &str, key: &str) -> Result<(), ContractError> {
    if sha.len() == 64 && is_lower_hex(sha) {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_sha256"))
    }
}

/// Check lowercase hex.
pub(crate) fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Check a `YYYY-MM-DD` review date (range-checked, not calendar-exact).
fn is_review_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    let month: u32 = parts[1].parse().unwrap_or(0);
    let day: u32 = parts[2].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}
