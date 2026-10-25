mod archive;
mod archive_tar_framing;
mod graph;
mod json;
mod layer;

use std::path::Path;

use archive::load_archive;
use graph::inspect_graph;

pub const MAX_OCI_CANDIDATE_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_OCI_CANDIDATE_ARCHIVE_MEMBERS: usize = 128;
pub const MAX_OCI_CANDIDATE_BLOB_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_OCI_CANDIDATE_LAYER_MEMBERS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciCandidateEvidence {
    pub archive_sha256: String,
    pub archive_size_bytes: u64,
    pub archive_index_digest: String,
    pub archive_index_size_bytes: u64,
    pub platform_manifest_digest: String,
    pub platform_manifest_size_bytes: u64,
    pub image_configuration_digest: String,
    pub image_configuration_size_bytes: u64,
    pub layer_digests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciCandidateError(&'static str);

impl std::fmt::Display for OciCandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for OciCandidateError {}

pub(crate) type Result<T> = std::result::Result<T, OciCandidateError>;

pub fn inspect_oci_candidate(path: &Path) -> Result<OciCandidateEvidence> {
    let archive = load_archive(path)?;
    inspect_graph(archive)
}
