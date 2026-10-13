//! Typed Rust release configuration (`[stacks.rust.release]`).
//!
//! Pure data plus validation: no subprocess, registry, or YAML access.
//! Unknown fields are rejected by serde; [`RustReleaseConfig::validate`]
//! reports file, key path, and problem for every other violation.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Release authentication mode (exactly one; modes never mix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAuthentication {
    /// crates.io Trusted Publishing via OIDC (`id-token: write` job).
    #[default]
    TrustedPublishing,
    /// Short-lived bootstrap API token for first publication only.
    BootstrapToken,
}

/// Exact first-publication authorization (bootstrap mode only).
///
/// Immutable record: package, version, and source SHA. A dispatch must never
/// widen this record; retire it after the OIDC handover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapRelease {
    /// Exact package authorized for first publication.
    pub package: String,
    /// Exact version authorized (`major.minor.patch`, numeric only).
    pub version: String,
    /// Full lowercase-hex immutable source SHA.
    pub source_sha: String,
}

/// Rust release policy (`[stacks.rust.release]`).
///
/// Disabled by default. There are no shell/YAML/`uses` override fields by
/// design; unknown keys fail deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustReleaseConfig {
    /// Publication enabled; `false` emits no release work.
    #[serde(default)]
    pub enabled: bool,
    /// Repo-relative workspace manifest (default `Cargo.toml`).
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,
    /// Explicit package allowlist; sorted, unique, never auto-expanded.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Explicit publishable-workspace opt-in; exclusive with `packages`.
    #[serde(default)]
    pub publishable_workspace: bool,
    /// Protected GitHub environment bound to publishing jobs.
    #[serde(default = "default_environment")]
    pub environment: String,
    /// Authentication mode (default trusted publishing).
    #[serde(default)]
    pub authentication: ReleaseAuthentication,
    /// Run release-plz release-pr preparation (default true).
    #[serde(default = "default_true")]
    pub release_pr: bool,
    /// Package-qualified tag template carrying package and version vars.
    #[serde(default = "default_tag_name")]
    pub tag_name: String,
    /// Bootstrap authorization; required iff bootstrap-token mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<BootstrapRelease>,
    /// Named version groups with non-lockstep semantics: members coordinate
    /// versioning through release-plz, but unchanged members are not forced
    /// to release. Members must be listed in `packages`; under the
    /// publishable-workspace opt-in, emission enforces set membership.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub version_groups: BTreeMap<String, Vec<String>>,
}

/// Default workspace manifest path.
fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

/// Default protected publishing environment name: `release`, matching the
/// crates.io trusted-publishing docs and majority ecosystem practice.
fn default_environment() -> String {
    "release".to_owned()
}

/// Default `release_pr` value.
fn default_true() -> bool {
    true
}

/// Default package-qualified tag template.
fn default_tag_name() -> String {
    "{{ package }}-v{{ version }}".to_owned()
}

impl Default for RustReleaseConfig {
    /// Disabled release with safe defaults (no packages selected).
    fn default() -> Self {
        Self {
            enabled: false,
            manifest_path: default_manifest_path(),
            packages: Vec::new(),
            publishable_workspace: false,
            environment: default_environment(),
            authentication: ReleaseAuthentication::default(),
            release_pr: true,
            tag_name: default_tag_name(),
            bootstrap: None,
            version_groups: BTreeMap::new(),
        }
    }
}

impl RustReleaseConfig {
    /// Validate shape, safety, and mode coherence.
    ///
    /// Runs whether enabled or not so drafted config stays safe; `enabled`
    /// additionally requires one scope: an allowlist or the publishable
    /// opt-in. Never expands either.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_manifest_path(file, &self.manifest_path)?;
        validate_packages(
            file,
            &self.packages,
            self.enabled,
            self.publishable_workspace,
        )?;
        validate_environment(file, &self.environment)?;
        self.validate_authentication(file)?;
        validate_tag_name(file, &self.tag_name)?;
        if let Some(bootstrap) = &self.bootstrap {
            validate_bootstrap(file, bootstrap)?;
        }
        validate_version_groups(
            file,
            &self.version_groups,
            &self.packages,
            self.publishable_workspace,
        )?;
        Ok(())
    }

    /// Reject contradictory authentication/authorization combinations.
    fn validate_authentication(&self, file: &str) -> Result<(), ContractError> {
        let key = "stacks.rust.release.bootstrap";
        match (self.authentication, &self.bootstrap) {
            (ReleaseAuthentication::BootstrapToken, None) => {
                Err(ContractError::config(file, key, "missing_bootstrap_record"))
            }
            (ReleaseAuthentication::TrustedPublishing, Some(_)) => Err(ContractError::config(
                file,
                key,
                "contradictory_authentication",
            )),
            _ => Ok(()),
        }
    }
}

/// Validate the workspace manifest path (relative, safe, `Cargo.toml`).
fn validate_manifest_path(file: &str, path: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.manifest_path";
    if path.is_empty() {
        return Err(ContractError::config(file, key, "empty_manifest_path"));
    }
    if !path.bytes().all(is_manifest_byte) {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_charset:{path}"),
        ));
    }
    if path.starts_with('/') || path.split('/').any(|seg| seg.is_empty() || seg == "..") {
        return Err(ContractError::config(file, key, "non_relative_manifest"));
    }
    if path != "Cargo.toml" && !path.ends_with("/Cargo.toml") {
        return Err(ContractError::config(file, key, "missing_cargo_toml"));
    }
    Ok(())
}

/// Validate the package scope (allowlist or publishable opt-in, never both).
fn validate_packages(
    file: &str,
    packages: &[String],
    enabled: bool,
    publishable: bool,
) -> Result<(), ContractError> {
    let key = "stacks.rust.release.packages";
    if publishable && !packages.is_empty() {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.publishable_workspace",
            "packages_with_publishable",
        ));
    }
    if enabled && packages.is_empty() && !publishable {
        return Err(ContractError::config(file, key, "empty_packages"));
    }
    let mut sorted = packages.to_vec();
    sorted.sort();
    if sorted.as_slice() != packages {
        return Err(ContractError::config(file, key, "must_be_sorted"));
    }
    let unique: BTreeSet<&str> = packages.iter().map(String::as_str).collect();
    if unique.len() != packages.len() {
        return Err(ContractError::config(file, key, "duplicate_package"));
    }
    for name in packages {
        if !is_package_name(name) {
            return Err(ContractError::config(
                file,
                key,
                format!("unsafe_package:{name}"),
            ));
        }
    }
    Ok(())
}

/// Validate the protected environment name.
fn validate_environment(file: &str, name: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.environment";
    if name.is_empty() {
        return Err(ContractError::config(file, key, "empty_environment"));
    }
    if name != name.trim() {
        return Err(ContractError::config(file, key, "padded_environment"));
    }
    if !name.bytes().all(is_environment_byte)
        || name.split('/').any(|seg| seg.is_empty() || seg == "..")
    {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_environment:{name}"),
        ));
    }
    Ok(())
}

/// Validate the tag template (safe charset, package and version vars).
fn validate_tag_name(file: &str, tag: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.release.tag_name";
    if tag.is_empty() || !tag.bytes().all(is_tag_byte) {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_charset:{tag}"),
        ));
    }
    if tag.matches("{{").count() != tag.matches("}}").count() {
        return Err(ContractError::config(file, key, "unbalanced_braces"));
    }
    let compact: String = tag.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("{{package}}") {
        return Err(ContractError::config(file, key, "missing_package_var"));
    }
    if !compact.contains("{{version}}") {
        return Err(ContractError::config(file, key, "missing_version_var"));
    }
    Ok(())
}

/// Validate one bootstrap authorization record (fail closed).
fn validate_bootstrap(file: &str, bootstrap: &BootstrapRelease) -> Result<(), ContractError> {
    if !is_package_name(&bootstrap.package) {
        let package = bootstrap.package.as_str();
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.package",
            format!("unsafe_package:{package}"),
        ));
    }
    if !is_bootstrap_version(&bootstrap.version) {
        let version = bootstrap.version.as_str();
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.version",
            format!("bad_version:{version}"),
        ));
    }
    if !is_full_sha(&bootstrap.source_sha) {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.bootstrap.source_sha",
            "bad_source_sha",
        ));
    }
    Ok(())
}

/// Validate version groups (known members, each in at most one group).
///
/// Under the publishable opt-in the allowlist is empty, so set membership
/// is deferred to emission; every other check still applies.
fn validate_version_groups(
    file: &str,
    groups: &BTreeMap<String, Vec<String>>,
    packages: &[String],
    publishable: bool,
) -> Result<(), ContractError> {
    let allowed: BTreeSet<&str> = packages.iter().map(String::as_str).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (group, members) in groups {
        let key = format!("stacks.rust.release.version_groups.{group}");
        if !is_package_name(group) {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.version_groups",
                format!("unsafe_group:{group}"),
            ));
        }
        if members.is_empty() {
            return Err(ContractError::config(file, key, "empty_group"));
        }
        let mut sorted = members.clone();
        sorted.sort();
        if sorted != *members {
            return Err(ContractError::config(file, key, "must_be_sorted"));
        }
        for member in members {
            if !is_package_name(member) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("unsafe_package:{member}"),
                ));
            }
            if !publishable && !allowed.contains(member.as_str()) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("unknown_package:{member}"),
                ));
            }
            if !seen.insert(member.as_str()) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("member_in_two_groups:{member}"),
                ));
            }
        }
        let unique: BTreeSet<&str> = members.iter().map(String::as_str).collect();
        if unique.len() != members.len() {
            return Err(ContractError::config(file, key, "duplicate_member"));
        }
    }
    Ok(())
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

/// Strict bootstrap version: exactly three dot-separated numeric parts.
fn is_bootstrap_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Full git SHA: 40 lowercase hex characters.
fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Bytes allowed in a repo-relative manifest path.
fn is_manifest_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_')
}

/// Bytes allowed in a protected environment name.
fn is_environment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'.')
}

/// Bytes allowed in a tag template (placeholders plus safe punctuation).
fn is_tag_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.' | b'/' | b'{' | b'}')
}
