//! Bounded inspection of the uncompressed OCI archive emitted by Buildx.

mod archive;
mod graph;
mod json;
mod layer;

#[cfg(test)]
#[path = "oci_candidate_tests.rs"]
mod tests;

use std::path::Path;

use archive::load_archive;
use graph::inspect_graph;

/// Maximum archive size on disk and total regular-file content.
pub const MAX_OCI_CANDIDATE_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Maximum number of visible archive entries after tar extension processing.
pub const MAX_OCI_CANDIDATE_ARCHIVE_MEMBERS: usize = 128;
/// Maximum bytes in one archive metadata file or layer/config blob.
pub const MAX_OCI_CANDIDATE_BLOB_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum total uncompressed layer bytes.
pub const MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Maximum number of path entries across all layers.
pub const MAX_OCI_CANDIDATE_LAYER_MEMBERS: usize = 1_000_000;

/// Exact OCI graph facts for one linux/amd64 archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciCandidateEvidence {
    /// SHA-256 of the complete archive bytes read from the authenticated descriptor.
    pub archive_sha256: String,
    /// Complete archive byte length read from the authenticated descriptor.
    pub archive_size_bytes: u64,
    /// Lowercase SHA-256 of `index.json` bytes.
    pub archive_index_digest: String,
    /// Length of `index.json`.
    pub archive_index_size_bytes: u64,
    /// Digest of the sole linux/amd64 platform manifest.
    pub platform_manifest_digest: String,
    /// Platform manifest byte length.
    pub platform_manifest_size_bytes: u64,
    /// Digest of the image configuration blob.
    pub image_configuration_digest: String,
    /// Image configuration byte length.
    pub image_configuration_size_bytes: u64,
    /// Ordered layer digests from the platform manifest.
    pub layer_digests: Vec<String>,
}

/// Bounded OCI archive validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciCandidateError(&'static str);

impl std::fmt::Display for OciCandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for OciCandidateError {}

pub(crate) type Result<T> = std::result::Result<T, OciCandidateError>;

/// Inspect archive structure, OCI reachability, blob digests, and layer `DiffIDs`.
///
/// # Errors
/// Rejects archives outside the fixed size, path, media-type, graph, or layer limits.
pub fn inspect_oci_candidate(path: &Path) -> Result<OciCandidateEvidence> {
    let archive = load_archive(path)?;
    inspect_graph(archive)
}
