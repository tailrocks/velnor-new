// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, canonical_json_bytes,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    package_release_plan::{PackageReleasePlan, PackageReleasePlanProduct},
};

pub const HOMEBREW_FORMULA_SPEC_SCHEMA_V1: &str = "velnor.homebrew-formula-spec.v1";

pub const HOMEBREW_FORMULA_TEMPLATE_V1: &str = "velnor-homebrew-standalone-v1";
const MAX_FORMULA_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewFormulaSpec {
    pub schema: String,

    pub template: String,

    pub description: String,

    pub homepage: String,

    pub license: String,

    pub channels: Vec<HomebrewFormulaChannelSpec>,

    pub host: HomebrewHostProductSpec,

    pub auxiliaries: Vec<HomebrewAuxiliaryProductSpec>,

    pub stable_sentinel: HomebrewStableSentinel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewFormulaChannelSpec {
    pub channel: PackageReleaseChannel,

    pub formula: String,

    pub formula_class: String,

    pub destination: String,

    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewHostProductSpec {
    pub product: String,

    pub targets: Vec<String>,

    pub bins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAuxiliaryProductSpec {
    pub product: String,

    pub targets: Vec<String>,

    pub files: Vec<HomebrewAuxiliaryFileSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewAuxiliaryFileSpec {
    pub target: String,

    pub member: String,

    pub destination: String,

    pub mode: HomebrewInstalledMode,

    pub test: HomebrewAuxiliaryTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomebrewInstalledMode {
    Data,

    Executable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomebrewAuxiliaryTest {
    None,

    Executable,

    LinuxElf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewStableSentinel {
    pub date: String,

    pub reason: String,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewFormulaAssetLiteral {
    name: String,
    url: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedHomebrewProjection {
    channel: PackageReleaseChannel,
    repository: String,
    base_version: String,
    selected: Vec<HomebrewSelectedPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewSelectedPayload {
    product: String,
    target: String,
    asset_name: String,
    members: Vec<String>,
    role: HomebrewPayloadRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomebrewPayloadRole {
    Host,

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
