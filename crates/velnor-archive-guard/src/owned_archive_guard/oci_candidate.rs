// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod archive;
mod archive_tar_framing;
mod config;
mod graph;
mod json;
mod layer;
mod materialize;

use std::path::{Path, PathBuf};

use archive::load_archive;
pub use config::OciImageConfiguration;
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
pub struct OciCandidateInspection {
    pub evidence: OciCandidateEvidence,
    pub configuration: OciImageConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciCandidateError(&'static str);

impl std::fmt::Display for OciCandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for OciCandidateError {}

#[derive(Debug)]
pub enum OciMaterializeError {
    Candidate(OciCandidateError),
    CleanupFailure { destination: PathBuf },
}

impl std::fmt::Display for OciMaterializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Candidate(error) => std::fmt::Display::fmt(error, f),
            Self::CleanupFailure { destination } => {
                write!(f, "oci_projection_cleanup: {}", destination.display())
            }
        }
    }
}

impl std::error::Error for OciMaterializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Candidate(error) => Some(error),
            Self::CleanupFailure { .. } => None,
        }
    }
}

impl From<OciCandidateError> for OciMaterializeError {
    fn from(error: OciCandidateError) -> Self {
        Self::Candidate(error)
    }
}

pub(crate) type Result<T> = std::result::Result<T, OciCandidateError>;

pub fn inspect_oci_candidate(path: &Path) -> Result<OciCandidateEvidence> {
    let archive = load_archive(path)?;
    inspect_graph(archive)
}

pub fn inspect_oci_candidate_with_config(path: &Path) -> Result<OciCandidateInspection> {
    let archive = load_archive(path)?;
    graph::inspect_graph_with_config(archive)
}

pub fn materialize_oci_candidate(
    archive: &Path,
    destination: &Path,
    expected: &OciCandidateEvidence,
) -> std::result::Result<OciCandidateEvidence, OciMaterializeError> {
    materialize::materialize_oci_candidate(archive, destination, expected)
}
