//! Rust/Cargo stack discovery and task proposals.
//!
//! Pure inventory, evidence, and task-group derivation from bytes the
//! orchestrator supplies. This crate launches no processes, builds no tool
//! invocations, reads no tool files, and renders no workflow text.

pub mod detect;
pub mod evidence;
pub mod graph;
pub mod index;
pub mod metadata;
pub mod scan;
pub mod tasks;

pub use detect::{
    CandidateOutcome, CargoCandidate, DetectError, DetectedProject, DetectionStatus,
    IGNORED_REASON, REGISTERED_STACKS, apply_stack_ignores, check_candidate_outcomes,
    check_duplicates, discover_candidates, project_root_for_manifest, selected_projects,
    to_detected_projects,
};
pub use evidence::{
    CompileDriver, Evidence, EvidenceFile, EvidenceStrength, NEXTEST_RECOMMENDATION,
    PERSIST_EVIDENCE, ProfileError, ProfileInputs, ProfileOutcome, Recommendation,
    RustExecutionProfile, TestRunner, detect_profile, evidence_scan_excluded, is_generated_output,
};
pub use graph::{dedupe_workspaces, reverse_closure};
pub use index::{
    BUILTIN_EXCLUSIONS, FileIndex, IndexError, build_index, build_index_from_list,
    build_index_walk, is_excluded, matches_glob, validate_pattern,
};
pub use metadata::{
    DepKind, LocalEdge, METADATA_FORMAT_VERSION, MetadataError, PackageRecord, TargetRecord,
    WorkspaceRecord, parse_metadata_json,
};
pub use tasks::{DeriveInputs, TaskGroup, TaskKind, derive_task_groups, derive_workspace_fmt};

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";
