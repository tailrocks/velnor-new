//! Strict handoff proof for an isolated tap candidate.

/// Proof retained for an isolated tap preparation and handed to the writer.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapPreparationProof {
    /// Versioned schema for the unprivileged candidate handoff.
    pub schema: String,
    /// Whether preparation produced a new formula or found this exact release
    /// already represented by the captured tap main.
    pub disposition: TapPreparationDisposition,
    /// Captured immutable tap-main commit.
    pub base_sha: String,
    /// Only permitted changed path.
    pub formula_path: String,
    /// SHA-256 of formula bytes at captured base.
    pub old_formula_sha256: String,
    /// SHA-256 of the independently validated candidate bytes.
    pub new_formula_sha256: String,
    /// Canonical digest of the authenticated preview release receipt.
    pub receipt_digest: String,
    /// Canonical digest of the static package release plan.
    pub plan_digest: String,
    /// Canonical digest of the tap policy used to prepare the candidate.
    pub tap_config_digest: String,
    /// Tap repository slug bound to the captured remote.
    pub tap_repository: String,
    /// Source repository whose immutable release is being promoted.
    pub source_repository: String,
    /// Exact immutable source commit from the receipt.
    pub source_sha: String,
    /// Deterministic preview version from the receipt.
    pub version: String,
    /// Exclusive path of the validated formula bytes for the isolated writer.
    pub candidate_formula: String,
}

/// Terminal result of preparing one release against captured tap main.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapPreparationDisposition {
    /// The formula differs from the captured base and requires a pull request.
    Changed,
    /// Captured main already contains the exact canonical formula for this release.
    FormulaAlreadyCurrent,
}
