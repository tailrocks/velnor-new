// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::package_release::PackageReleaseChannel;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapPreparationProof {
    pub schema: String,
    pub channel: PackageReleaseChannel,
    pub disposition: TapPreparationDisposition,
    pub base_sha: String,
    pub formula_path: String,
    pub old_formula_sha256: String,
    pub new_formula_sha256: String,
    pub receipt_digest: String,
    pub plan_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_spec_digest: Option<String>,
    pub sdk_artifact_digest: String,
    pub tap_config_digest: String,
    pub tap_repository: String,
    pub source_repository: String,
    pub source_sha: String,
    pub version: String,
    pub candidate_formula: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapPreparationDisposition {
    Changed,
    FormulaAlreadyCurrent,
}
