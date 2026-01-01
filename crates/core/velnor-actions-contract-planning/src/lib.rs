//! Plan construction: project detection, task proposal, task graphs.
//!
//! Owns the file index, stack detectors and their registry, task
//! proposals for detected units, and task-graph validation. Must not own
//! identities, configuration, release manifests, or workflow IR. Built
//! on `velnor-actions-contract` (identifiers) and
//! `velnor-actions-contract-config` (stack registry, selection).

pub mod discover;
pub mod graph;
pub mod propose;

pub use discover::{
    BUILTIN_EXCLUSIONS, DETECTION_SCHEMA, DetectError, DetectedProject, DetectionStatus,
    DetectorEntry, FileIndex, IGNORED_REASON, IndexError, IndexMode, apply_stack_ignores,
    build_index, build_index_from_list, build_index_from_tracked, build_index_walk,
    check_duplicates, is_excluded, is_reserved_cache_path_bytes, matches_glob, reverse_closure,
    selected_projects, validate_pattern,
};
pub use graph::{
    CachePolicy, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph, TaskNode,
    validate_plan_edges,
};
pub use propose::{
    CandidateOutcome, IdentityInputs, ProposedTask, StackCandidate, check_candidate_outcomes,
    component_id_for_unit, project_root_for_unit_path,
};
