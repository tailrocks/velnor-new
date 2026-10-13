//! Rust/Cargo stack discovery and task proposals.
//!
//! Pure inventory, evidence, and task-group derivation from bytes the
//! orchestrator supplies. This crate launches no processes, builds no tool
//! invocations, reads no files itself, and renders no workflow text. It owns
//! read-only inspection of supplied `rust-toolchain.toml` bytes.

mod argv;
pub mod cargo_env;
pub mod detect;
pub mod evidence;
mod evidence_text;
pub mod graph;
pub mod identity;
pub mod index;
mod index_glob;
pub mod metadata;
pub mod metadata_edges;
pub mod profile;
mod profile_select;
pub mod release_config;
pub mod release_error;
pub mod release_facts;
pub mod release_graph;
pub mod release_select;
pub mod release_semver;
pub mod scan;
pub mod stability;
pub mod tasks;
pub mod toolfiles;
pub mod tracked;

pub use cargo_env::{DENY_WARNINGS, RUSTDOCFLAGS_ENV, cargo_payload_env};
pub use detect::{
    CandidateOutcome, CargoCandidate, DetectError, DetectedProject, DetectionStatus,
    IGNORED_REASON, REGISTERED_STACKS, apply_stack_ignores, check_candidate_outcomes,
    check_duplicates, discover_candidates, project_root_for_manifest, selected_projects,
    to_detected_projects,
};
pub use evidence::{
    Evidence, EvidenceFile, EvidenceStrength, MiseWrapperInput, NEXTEST_RECOMMENDATION,
    NextestConfigInput, PERSIST_EVIDENCE, SHADOWED_NEXTEST_CONFIG, evidence_scan_excluded,
    is_generated_output,
};
pub use graph::{dedupe_workspaces, reverse_closure};
pub use identity::{GroupExtensionInputs, adapter_entry_metadata, expand_shards_for_group};
pub use index::{
    BUILTIN_EXCLUSIONS, FileIndex, IndexError, build_index, build_index_from_list, build_index_walk,
};
pub use index_glob::{is_excluded, matches_glob, validate_pattern};
pub use metadata::{
    METADATA_FORMAT_VERSION, MetadataError, PackageRecord, TargetRecord, WorkspaceRecord,
    parse_metadata_json,
};
pub use metadata_edges::{DepKind, LocalEdge, SkippedPathEdge};
pub use profile::{
    AMBIGUOUS_DRIVER_CODE, AMBIGUOUS_RUNNER_CODE, CompileDriver, NextestProfile,
    PROFILE_CONFLICT_CODE, ProfileError, ProfileFinding, ProfileInputs, ProfileOutcome,
    ProfileSource, Recommendation, RustExecutionProfile, TRANSIENT_EVIDENCE_CODE, TestRunner,
    detect_profile,
};
pub use release_config::{
    DEFAULT_TAG_PATTERN, EmitOptions, ExistingTag, TagOutcome, TagState, VersionGroup,
    classify_tag, emit_bootstrap_config, emit_release_plz_config, resolve_version_groups,
};
pub use release_error::ReleaseError;
pub use release_facts::{DepFact, DepSource, PublishSetting, ReleaseFacts};
pub use release_graph::{PackagingEdge, PublicationGraph, RegistryState, publication_graph};
pub use release_select::{
    DEFAULT_REGISTRY, ReleaseRequest, ReleaseScope, ReleaseSelection, ResolvedScope,
    SelectedPackage, select_release_set,
};
pub use release_semver::{
    PreIdent, SemVersion, VersionReq, parse_req, parse_version, req_matches, version_satisfies,
};
pub use stability::{
    CommittedProfile, committed_profile_differs, read_committed_profile_for_comparison,
};
pub use tasks::{
    DeriveInputs, TaskGroup, TaskKind, derive_task_groups, derive_workspace_fmt,
    derive_workspace_fmt_if_explicit,
};
pub use toolfiles::{
    FOREIGN_TOOL_FILES, MISSING_RECOMMENDED_INPUT, OWNED_SYMBOLS, RUST_TOOLCHAIN_FILE,
    TOOLING_INPUT_INVALID, ToolFile, ToolFinding, ToolInspectError, ToolchainInspection,
    ToolchainSpec, inspect_toolchain_file, is_owned_tool_file, stack_for_symbol,
};
pub use tracked::{IndexMode, build_index_from_tracked};

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";
