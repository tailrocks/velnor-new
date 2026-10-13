//! Rust/Cargo stack discovery and task proposals.
//!
//! Pure inventory, evidence, and task-group derivation from bytes the
//! orchestrator supplies. This crate launches no processes, builds no tool
//! invocations, reads no files itself, and renders no workflow text. It owns
//! read-only inspection of supplied `rust-toolchain.toml` bytes.

mod argv;
pub mod detect;
pub mod evidence;
pub mod graph;
pub mod index;
pub mod metadata;
pub mod scan;
pub mod tasks;
pub mod toolfiles;

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
pub use toolfiles::{
    FOREIGN_TOOL_FILES, MISSING_RECOMMENDED_INPUT, OWNED_SYMBOLS, RUST_TOOLCHAIN_FILE,
    TOOLING_INPUT_INVALID, ToolFile, ToolFinding, ToolInspectError, ToolchainInspection,
    ToolchainSpec, inspect_toolchain_file, is_owned_tool_file, stack_for_symbol,
};

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";
