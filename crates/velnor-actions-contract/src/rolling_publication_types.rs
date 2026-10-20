//! Authenticated observations and durable identities for rolling publication.

use super::PublicationIdentity;
use std::collections::BTreeMap;

/// Closed GitHub upload states admitted by the rolling transport parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollingAssetState {
    /// Completed upload with positive size and an exact server digest.
    Uploaded,
    /// Incomplete upload left by an interrupted server operation.
    Starter,
}

/// An exact asset observation from the configured release, including starters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollingAsset {
    /// Immutable GitHub asset database ID; deletion never selects by name alone.
    pub id: u64,
    /// Observed server filename.
    pub name: String,
    /// Exact admitted GitHub state; unknown states fail before planning.
    pub state: RollingAssetState,
    /// Uploaded size; an unfinished server-side starter may have size zero.
    pub size: u64,
    /// Server SHA-256 for completed uploads; absent for unfinished starters.
    pub sha256: Option<String>,
}

/// Retained immutable artifact containing the independently signed generation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollingOriginal {
    /// Original workflow run identity.
    pub run_id: u64,
    /// Original complete attempt.
    pub run_attempt: u32,
    /// Exact immutable workflow artifact database ID.
    pub artifact_id: u64,
    /// SHA-256 of the complete service archive.
    pub archive_sha256: String,
}

/// Durable replacement phase; no upload precedes its staging intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollingPhase {
    /// Signed original and predecessor closure recorded before uploads.
    Staging,
    /// Complete staged IDs frozen before predecessor deletion.
    Replacing,
    /// Exact canonical inventory and tag re-observed.
    Committed,
}

/// Recovery journal, independently checked against signed bytes and live IDs.
///
/// This document is not an authorization token. Its identity must match the
/// authenticated original generation; all recorded objects are re-observed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollingJournal {
    /// Original signed receipt and complete desired inventory.
    pub identity: PublicationIdentity,
    /// Independently authenticated retained original; never authority by itself.
    pub original: RollingOriginal,
    /// Durable phase of this exact generation.
    pub phase: RollingPhase,
    /// Complete staged objects, keyed by their final canonical names.
    pub staged: BTreeMap<String, RollingAsset>,
    /// Exact predecessor objects captured before replacement starts.
    pub obsolete: Vec<RollingAsset>,
}

/// Release lifecycle classified from independently observed GitHub flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollingReleaseStatus {
    /// Existing published mutable prerelease; the only editable rolling state.
    MutablePreview,
    /// Immutable release protection forbids editing.
    Immutable,
    /// Drafts cannot become the grandfathered rolling release.
    Draft,
    /// Other or missing release metadata is not rolling preview authority.
    Other,
}

/// Independently observed state of the one existing rolling release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollingState {
    /// Independently authenticated original candidate archive.
    pub original: RollingOriginal,
    /// Actual existing release ID, never an instruction to create another one.
    pub release_id: u64,
    /// The single configured moving channel tag.
    pub tag: String,
    /// Exact classified lifecycle; only an existing mutable preview is admitted.
    pub status: RollingReleaseStatus,
    /// Actual peeled tag source; `release.target_commitish` is not authoritative.
    pub tag_source: String,
    /// True only after an authenticated GitHub ancestry comparison.
    pub tag_can_fast_forward: bool,
    /// Complete paginated asset inventory, including unfinished uploads.
    pub assets: Vec<RollingAsset>,
    /// Independently admitted predecessor or explicit bootstrap snapshot.
    /// Observation alone never grants permission to remove an unrelated asset.
    pub replaceable: Vec<RollingAsset>,
    /// Validated native journal, if this release already has one.
    pub journal: Option<RollingJournal>,
    /// Authenticated complete previous receipt, when present.
    pub published: Option<PublicationIdentity>,
    /// Whether an older journal's source is an ancestor of this candidate.
    pub journal_can_advance: bool,
}
