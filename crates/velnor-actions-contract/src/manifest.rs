//! Release-manifest and `generator.lock` schemas plus per-target records.
//!
//! Serde shapes only; file IO and TOML parsing live outside this crate.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest_checks::{
    check_commit, check_immutable_url, check_schema, check_semver, check_sha256, check_target,
    is_lower_hex, is_review_date,
};

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
    /// Full 40-char source commit SHA (lowercase hex) the release was cut from.
    ///
    /// Required (F3): manifests without it are rejected at parse, and
    /// malformed values fail validation. The Acquire step records this
    /// commit so reviewers can verify the pinned source.
    pub commit: String,
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
    /// Full 40-char source commit (lowercase hex) the locked binaries came from.
    ///
    /// Required (F3): same strictness as the release manifest — absent at
    /// parse fails the lock read, malformed fails validation. The
    /// lock-backed Acquire records it as `VELNOR_RELEASE_COMMIT`.
    pub commit: String,
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
        Self::parse_json_with_limit(text, file, crate::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES)
    }

    /// Parse manifest JSON with an explicit per-caller size bound in bytes.
    /// # Errors
    pub fn parse_json_with_limit(
        text: &str,
        file: &str,
        limit: usize,
    ) -> Result<Self, ContractError> {
        let value =
            crate::strict_json::parse_strict_json_with_limit(text, limit).map_err(|err| {
                ContractError::config(file, "document", format!("malformed_json:{err}"))
            })?;
        serde_json::from_value(value).map_err(|err| map_manifest_error(file, &err))
    }

    /// Find the asset record for a target triple.
    #[must_use]
    pub fn record_for_target(&self, target: &str) -> Option<&TargetRecord> {
        self.targets.iter().find(|record| record.target == target)
    }

    /// Validate the separate published asset URL against this manifest.
    ///
    /// # Errors
    pub fn validate_published_asset_url(&self, url: &str, file: &str) -> Result<(), ContractError> {
        crate::targets::check_release_manifest_artifact(
            url,
            &self.version,
            &self.commit,
            file,
            "manifest_asset",
        )
    }

    /// Validate schema, version, repository, and exactly one record per
    /// supported target.
    ///
    /// The repository is pinned to the canonical identity and every
    /// artifact URL is bound to this exact version and target (X1); a
    /// shape-only URL here would let a merged manifest redirect the
    /// Acquire step at attacker infrastructure.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        check_semver(&self.version, file, "version")?;
        if self.repository != crate::targets::EXPECTED_REPOSITORY {
            return Err(ContractError::config(
                file,
                "repository",
                "unexpected_repository",
            ));
        }
        check_commit(&self.commit, file, "commit")?;
        if self.targets.is_empty() {
            return Err(ContractError::config(file, "targets", "empty_targets"));
        }
        let mut seen = BTreeSet::new();
        for record in &self.targets {
            check_target(&record.target, file, "targets.target")?;
            if !crate::targets::is_supported_target(&record.target) {
                return Err(ContractError::config(
                    file,
                    "targets",
                    format!("unsupported_target:{}", record.target),
                ));
            }
            if !seen.insert(record.target.as_str()) {
                return Err(ContractError::config(
                    file,
                    "targets",
                    format!("duplicate_target:{}", record.target),
                ));
            }
            crate::targets::check_release_artifact(
                &record.artifact,
                &self.version,
                &self.commit,
                &record.target,
                file,
                "targets.artifact",
            )?;
            check_sha256(&record.sha256, file, "targets.sha256")?;
        }
        for target in crate::targets::ReleaseTarget::ALL {
            if !seen.contains(target.triple()) {
                return Err(ContractError::config(
                    file,
                    "targets",
                    format!("missing_target:{}", target.triple()),
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
        for target in crate::targets::ReleaseTarget::ALL {
            if self.binary_for_target(target.triple()).is_none() {
                return Err(ContractError::config(
                    file,
                    "generator.binaries",
                    format!("missing_target:{}", target.triple()),
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
        check_commit(&self.generator.commit, file, "generator.commit")?;
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
