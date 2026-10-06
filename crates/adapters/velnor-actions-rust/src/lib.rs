//! Rust/Cargo stack discovery and task proposals.
//!
//! Pure inventory, evidence, and task-group derivation from bytes the
//! orchestrator supplies. This crate launches no processes, builds no tool
//! invocations, reads no files itself, and renders no workflow text. It owns
//! read-only inspection of supplied `rust-toolchain.toml` bytes.

mod argv;
pub mod cargo_env;
pub mod closure;
mod closure_probes;
pub mod detect;
pub mod evidence;
mod evidence_text;
pub mod graph;
pub mod identity;
mod manifest_edges;
pub mod metadata;
pub mod metadata_edges;
pub mod profile;
mod profile_select;
pub mod propose;
pub mod release_config;
pub mod release_error;
pub mod release_facts;
pub mod release_graph;
pub mod release_select;
pub mod release_semver;
pub mod scan;
pub mod stability;
mod task_identity;
pub mod tasks;
pub mod toolfiles;

pub use cargo_env::{DENY_WARNINGS, RUSTDOCFLAGS_ENV, cargo_payload_env};
pub use closure::{lock_digest_at_root, nextest_digest_at_root, resolve_closure_at_root};
pub use detect::{
    CargoCandidate, detected_projects_for_units, discover_candidates, discover_stack_candidates,
    manifest_for_key, manifest_for_unit_root, project_root_for_manifest,
};
pub use evidence::{
    Evidence, EvidenceFile, EvidenceStrength, MiseWrapperInput, NEXTEST_RECOMMENDATION,
    NextestConfigInput, PERSIST_EVIDENCE, SHADOWED_NEXTEST_CONFIG, evidence_scan_excluded,
    is_generated_output,
};
pub use graph::dedupe_workspaces;
pub use identity::{
    GroupExtensionInputs, adapter_entry_metadata, entry_metadata_for_task, expand_shards_for_group,
    extension_for_proposal,
};
pub use manifest_edges::manifest_edges;
pub use metadata::{
    METADATA_FORMAT_VERSION, MetadataError, PackageRecord, TargetRecord, WorkspaceRecord,
    parse_metadata_json,
};
pub use metadata_edges::{DepKind, LocalEdge, SkippedPathEdge, local_edge_pairs};
pub use profile::{
    AMBIGUOUS_DRIVER_CODE, AMBIGUOUS_RUNNER_CODE, CompileDriver, NextestProfile,
    PROFILE_CONFLICT_CODE, ProfileError, ProfileFinding, ProfileInputs, ProfileOutcome,
    ProfileSource, Recommendation, RustExecutionProfile, TRANSIENT_EVIDENCE_CODE, TestRunner,
    detect_profile,
};
pub use propose::{
    KIND_DISPLAY_WORDS, ToolNeeds, is_clippy_kind, is_nextest_kind, is_workspace_fmt_task,
    payload_env_for_kind, propose_task, resource_class_for_kind, step_base_name, task_kind_rank,
    tool_needs,
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
    SelectionBroadening, TOOLING_INPUT_INVALID, ToolFile, ToolFinding, ToolInspectError,
    ToolchainInspection, ToolchainSpec, inspect_toolchain_file, is_known_toolfile,
    is_owned_tool_file, selection_broadening, stack_for_symbol,
};

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";
