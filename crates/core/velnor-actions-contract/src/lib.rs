//! Task identity, canonical digests, and identifier vocabulary.
//!
//! Owns stable identity derivation (canonical JSON, BLAKE3 digests, task
//! closures), every validated identifier (task, run, plan, report,
//! artifact, matrix, job), cache keys, VCS revision inputs, secret-name
//! screening, known-stack vocabulary, and the generator marker.
//!
//! Must not own configuration, release manifests, detection, proposals,
//! graphs, workflow IR, plans, or reports: those live in the sibling
//! `velnor-actions-contract-*` crates, which all build on this leaf.
//!
//! All types here are effect-free data plus pure derivation/validation.
//! Derivation formulas are normative; example strings in docs are illustrative.

pub mod archive;
pub mod cachekey;
pub mod canonical;
pub mod closure;
pub mod errors;
pub mod extension_schemas;
pub mod ids;
pub mod marker;
pub mod secrets;
pub mod stack;
pub mod strict_json;
pub mod vcs;

pub use archive::{ArchiveInputs, archive_id};
pub use canonical::{
    CompatibilityInputs, Digest, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3, input_digest,
    is_valid_digest, normalize_posix_path, validate_digest,
};
pub use closure::{ClosureBuilder, Provenance, TaskInputClosure};
pub use errors::{ContractError, sanitize_error_detail};
pub use extension_schemas::NAMED_CHECK_EXTENSION_SCHEMA;
pub use ids::job_ids::{
    CRATE_JOB_ID_PREFIX, TOFU_JOB_ID_PREFIX, assign_crate_job_ids, is_crate_job_id,
    slugify_segment, validate_job_id,
};
pub use ids::{
    artifact_id_for_baseline, artifact_id_for_crate_job, artifact_id_for_final,
    artifact_id_for_matrix, artifact_id_for_plan, manifest_key_for_cargo_manifest,
    matrix_id_for_task_group, matrix_key_for_id, plan_id_for_run, report_id_for_matrix,
    run_key_for_ci, split_shard_suffix, target_key, task_id_for_internal, task_id_for_stack,
    task_report_id_for_task, validate_artifact_id, validate_fetch_root, validate_id,
    validate_matrix_key, validate_plan_id, validate_report_id, validate_run_key, validate_task_id,
    validate_task_report_id,
};
pub use marker::{MARKER_PREFIX, OLD_MARKER_PREFIX, is_generated_marker_line};
pub use secrets::is_secret_env_name;
pub use stack::Stack;
pub use strict_json::parse_strict_json;
pub use vcs::VcsInputs;

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;
