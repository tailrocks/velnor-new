//! Stack-neutral workflow/task contracts.
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.
//!
//! All types here are effect-free data plus pure derivation/validation; derivation formulas are normative.

pub mod archive;
pub mod cachekey;
pub mod candidate_manifest;
pub mod canonical;
pub mod closure;
pub mod config;
mod config_reexports;
pub mod deps_lock;
pub mod discover;
pub mod errors;
mod exports;
pub mod extension_schemas;
pub mod extensions;
pub mod finding;
pub mod formats;
pub mod freshness;
pub mod github_evidence_snapshot;
pub mod graph;
pub mod homebrew_adoption;
pub mod homebrew_authority;
pub mod homebrew_formula;
pub mod homebrew_merge_gate;
pub mod homebrew_review_completion;
pub mod homebrew_spec;
pub mod ids;
mod image_candidate;
pub mod manifest;
pub(crate) mod manifest_checks;
pub mod marker;
pub mod package_release;
pub mod package_release_plan;
pub mod package_release_policy;
pub mod policy;
pub mod preview_identity;
pub mod preview_production;
pub mod propose;
pub mod publication_retry;
mod release_reexports;
pub mod review_completion;
pub mod secrets;
pub mod strict_json;
pub mod targets;
pub mod tooling;
pub mod vcs;
pub mod workflow;
mod workflow_reexports;
pub use config::{
    DEFAULT_IMAGE_TIMEOUT_MINUTES, GithubConfig, IMAGE_JOB_PREFIX, IMAGE_REGISTRY_HOST,
    ImageConfig, ImageDockerfile, ImagePlatform, ImagePrebuilt, PrivateAssetSource,
    PrivateGitTreeSource, PrivateSourcesConfig, ReleaseRegistry, UnmanagedPolicy,
};
pub use config_reexports::*;
pub use exports::*;
pub use github_evidence_snapshot::{
    GITHUB_EVIDENCE_SNAPSHOT_SCHEMA, GitHubEvidenceAcquisitionV1, GitHubEvidenceArtifactCaptureV1,
    GitHubEvidenceArtifactRefV1, GitHubEvidenceDocumentKindV1, GitHubEvidenceDocumentV1,
    GitHubEvidenceFetchSpecV1, GitHubEvidenceRepositoryV1, GitHubEvidenceRunAttemptV1,
    GitHubEvidenceSnapshotV1, GitHubEvidenceWorkflowRefV1, GitHubEvidenceWorkflowSourceRefV1,
    MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES, MAX_GITHUB_EVIDENCE_ARCHIVES, MAX_GITHUB_EVIDENCE_ATTEMPTS,
    MAX_GITHUB_EVIDENCE_BUNDLE_BYTES, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES,
    MAX_GITHUB_EVIDENCE_DOCUMENTS, is_valid_github_evidence_conclusion,
    is_valid_github_evidence_status,
};
pub use image_candidate::*;
pub use release_reexports::*;
