#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
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
pub mod secrets;
pub mod strict_json;
pub mod targets;
pub mod tooling;
pub mod vcs;
pub mod workflow;
mod workflow_reexports;
pub use archive::{ArchiveInputs, archive_id};
pub use canonical::{
    CompatibilityInputs, Digest, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3, input_digest,
    is_valid_digest, normalize_posix_path, validate_digest,
};
pub use closure::{ClosureBuilder, Provenance, TaskInputClosure};
pub use config_reexports::*;
pub use discover::{
    BUILTIN_EXCLUSIONS, DETECTION_SCHEMA, DetectError, DetectedProject, DetectionStatus,
    DetectorEntry, FileIndex, IGNORED_REASON, IndexError, IndexMode, Stack, apply_stack_ignores,
    build_index, build_index_from_list, build_index_from_tracked, build_index_walk,
    check_duplicates, is_excluded, is_reserved_cache_path_bytes, matches_glob, reverse_closure,
    selected_projects, validate_pattern,
};
pub use errors::{ContractError, sanitize_error_detail};
pub use extension_schemas::NAMED_CHECK_EXTENSION_SCHEMA;
pub use extension_schemas::named_check::{
    NamedCheckIdentityExtension, validate_named_check_extension,
};
pub use extensions::{
    RUST_EXTENSION_REQUIRED_SLOTS, TOFU_EXTENSION_REQUIRED_SLOTS, validate_rust_extension,
    validate_tofu_extension,
};
pub use finding::Finding;
pub use formats::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_POINTER_BODY, DECLARED_GITHUB_FORMATS,
    find_github_format, is_declared_github_format,
};
pub use freshness::{
    FRESHNESS_CLASSES, FreshnessRequirement, RunnerImageEvidence, UNOBSERVED_IMAGE_VALUE,
    runner_family_changed, validate_freshness_class,
};
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
pub use graph::{
    CachePolicy, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph, TaskNode,
    validate_plan_edges,
};
pub use ids::{
    artifact_id_for_crate_job, artifact_id_for_final, artifact_id_for_matrix, artifact_id_for_plan,
    is_valid_branch_name, manifest_key_for_cargo_manifest, matrix_id_for_task_group,
    matrix_key_for_id, plan_id_for_run, report_id_for_matrix, run_key_for_ci, split_shard_suffix,
    target_key, task_id_for_internal, task_id_for_stack, task_report_id_for_task,
    validate_artifact_id, validate_fetch_root, validate_id, validate_matrix_key, validate_plan_id,
    validate_report_id, validate_run_key, validate_task_id, validate_task_report_id,
};
pub use manifest::{
    ActionPin, CandidateArtifactManifest, GeneratorBinary, GeneratorLock, LockedGenerator,
    MiseBootstrap, ReleaseManifest, TargetRecord, require_release_version,
};
pub use marker::{MARKER_PREFIX, OLD_MARKER_PREFIX, is_generated_marker_line};
pub use policy::{
    FreshnessEntry, FreshnessStatus, GithubRunnerImages, NightlyRecord, PolicyException,
    RunnerInventory, VersionPolicy, days_between,
};
pub use propose::{
    CandidateOutcome, IdentityInputs, ProposedTask, StackCandidate, check_candidate_outcomes,
    component_id_for_unit, project_root_for_unit_path,
};
pub use release_reexports::*;
pub use secrets::is_secret_env_name;
pub use strict_json::parse_strict_json;
pub use targets::{
    CONTRACT_VERSION, EXPECTED_REPOSITORY, RELEASE_MANIFEST_FILENAME, ReleaseTarget,
    SUPPORTED_TARGETS, asset_filename, check_release_artifact, is_seed_tag_for_version,
    is_supported_target,
};
pub use tooling::ToolIdentity;
pub use vcs::VcsInputs;
pub use workflow_reexports::*;

pub mod review_completion;

mod image_candidate;
pub use image_candidate::*;
