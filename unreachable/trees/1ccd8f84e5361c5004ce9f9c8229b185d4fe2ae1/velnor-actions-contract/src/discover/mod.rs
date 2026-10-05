//! Stack-neutral discovery: file index plus detection records.
//!
//! One bounded repository file index feeds every detector; detection
//! records, selection, and the detector registry contract live here.
//! Marker recognition and unit interpretation stay in stack adapters.

mod glob;
mod index;
mod project;
mod registry;
mod tracked;

pub use glob::{is_excluded, matches_glob, validate_pattern};
pub use index::{
    BUILTIN_EXCLUSIONS, FileIndex, IndexError, build_index, build_index_from_list, build_index_walk,
};
pub use project::{
    DetectError, DetectedProject, DetectionStatus, IGNORED_REASON, apply_stack_ignores,
    check_duplicates, reverse_closure, selected_projects,
};
pub use registry::{DETECTION_SCHEMA, DetectorEntry, Stack};
pub use tracked::{IndexMode, build_index_from_tracked};
