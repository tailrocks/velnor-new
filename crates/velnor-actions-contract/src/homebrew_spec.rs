//! Canonical, typed policy for standalone Homebrew formula generation.
//!
//! This module owns data validation and deterministic rendering only. It does
//! not read files, execute Homebrew, inspect YAML, or authenticate releases.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, canonical_json_bytes,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    package_release_plan::{PackageReleasePlan, PackageReleasePlanProduct},
};

/// Canonical formula policy schema.
pub const HOMEBREW_FORMULA_SPEC_SCHEMA_V1: &str = "velnor.homebrew-formula-spec.v1";
/// Fixed renderer template identity.
pub const HOMEBREW_FORMULA_TEMPLATE_V1: &str = "velnor-homebrew-standalone-v1";
const MAX_FORMULA_BYTES: usize = 1024 * 1024;

/// One repository's complete, canonical formula policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewFormulaSpec {
    /// Closed schema discriminator.
    pub schema: String,
    /// Fixed structural renderer identity.
    pub template: String,
    /// Human-readable formula metadata.
    pub description: String,
    /// HTTPS project page.
    pub homepage: String,
    /// SPDX license expression.
    pub license: String,
    /// Preview and stable destinations. Stable may be absent until enabled.
    pub channels: Vec<HomebrewFormulaChannelSpec>,
    /// One host executable product selected from the package release plan.
    pub host: HomebrewHostProductSpec,
    /// Additional data or executable products installed under `libexec`.
    pub auxiliaries: Vec<HomebrewAuxiliaryProductSpec>,
    /// Fixed, non-installable stable sentinel policy.
    pub stable_sentinel: HomebrewStableSentinel,
}

/// Channel-specific formula destination and conflict behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewFormulaChannelSpec {
    /// Release channel represented by this formula.
    pub channel: PackageReleaseChannel,
    /// Formula name for this channel.
    pub formula: String,
    /// Ruby formula class, checked against the fixed name-to-class derivation.
    pub formula_class: String,
    /// Exact formula path for the channel.
    pub destination: String,
    /// Existing formulas whose simultaneous installation conflicts.
    pub conflicts: Vec<String>,
}

/// Host executable selection from a signed release plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewHostProductSpec {
    /// Stable product ID in the release plan.
    pub product: String,
    /// Exact selected target triples, sorted and unique.
    pub targets: Vec<String>,
    /// Exact user-facing binary names, equal to the product's declared bins.
    pub bins: Vec<String>,
}

/// Auxiliary product selection and deterministic `libexec` placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAuxiliaryProductSpec {
    /// Stable product ID in the release plan.
    pub product: String,
    /// Exact selected target triples, sorted and unique.
    pub targets: Vec<String>,
    /// Installed relative files, sorted by target and then archive member.
    pub files: Vec<HomebrewAuxiliaryFileSpec>,
}

/// One archive member installed under the formula's `libexec` directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAuxiliaryFileSpec {
    /// Exact Rust target whose selected archive contains this member.
    pub target: String,
    /// Exact regular-file member name in each selected archive.
    pub member: String,
    /// Relative path under `libexec`.
    pub destination: String,
    /// Fixed installed mode.
    pub mode: HomebrewInstalledMode,
    /// Fixed offline post-install test.
    pub test: HomebrewAuxiliaryTest,
}

/// Closed installed file mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomebrewInstalledMode {
    /// Install as a regular non-executable file.
    Data,
    /// Install as an executable file.
    Executable,
}

/// Closed offline test for one auxiliary installed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomebrewAuxiliaryTest {
    /// No test beyond byte, mode, and installed-path identity checks.
    None,
    /// Require the installed file to be executable.
    Executable,
    /// Check ELF class, endianness, and machine without execution.
    LinuxElf,
}

/// Fixed disabled stable version and reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewStableSentinel {
    /// Stable sentinel date, `YYYY-MM-DD`.
    pub date: String,
    /// Fixed human-readable reason for the disabled formula.
    pub reason: String,
}

/// Formula bytes' release literals, validated against a spec and plan projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewFormulaReleaseLiterals {
    source_sha: Option<String>,
    version: String,
    tag: Option<String>,
    assets: Vec<HomebrewFormulaAssetLiteral>,
    channel: PackageReleaseChannel,
    repository: String,
    selected: Vec<HomebrewSelectedPayload>,
    spec_digest: String,
    disabled_stable_sentinel: bool,
}

/// Release URL and SHA-256 for one selected payload archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewFormulaAssetLiteral {
    name: String,
    url: String,
    sha256: String,
}

/// Private proof that spec selections resolve exactly against one release plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedHomebrewProjection {
    channel: PackageReleaseChannel,
    repository: String,
    base_version: String,
    selected: Vec<HomebrewSelectedPayload>,
}

/// Selected payload record, derived only from the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewSelectedPayload {
    product: String,
    target: String,
    asset_name: String,
    members: Vec<String>,
    role: HomebrewPayloadRole,
}

/// Selection role used by fixed renderer logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomebrewPayloadRole {
    /// Host executable archive.
    Host,
    /// Auxiliary archive.
    Auxiliary,
}

mod literals;
mod policy;
mod projection;
mod recognize;
mod render;
mod validation;

pub use projection::validate_homebrew_projection;
use projection::validate_projection_spec;
pub use recognize::parse_generated_formula;
pub use render::render_homebrew_formula;
use validation::{
    canonical_channel_version_matches, channel_key, formula_cpu_class, invalid, is_full_sha,
    is_sha256, release_version_matches_plan, ruby_assignment, ruby_class_name, ruby_cpu_predicate,
    ruby_platform_open, ruby_quote, safe_archive_member, safe_binary_name, safe_formula_name,
    safe_formula_path, safe_identifier, safe_relative_path, safe_release_tag, safe_version,
    sorted_unique_strings, supported_formula_target, unquote, valid_https_url, valid_iso_date,
    valid_ruby_string, valid_spdx_expression,
};

#[cfg(test)]
#[path = "homebrew_spec_tests.rs"]
mod tests;
