//! Generated-tree substrate: YAML values, markers, guards, size budgets.
//!
//! The document layer under workflow rendering: deterministic YAML values
//! and emission, first-line version markers, output-path guards, the
//! workflow byte budget, and the rendered-file envelopes assemblers share.

pub mod agents_md;
pub mod composite;
pub mod guard;
pub mod job_entries;
pub mod marker;
pub mod rendered;
pub mod runs_on;
pub mod steps_plain;
pub mod workflow_size;
pub mod yaml;

pub use guard::{SafeTreePath, check_no_symlink, join_within_root, validate_tree_path};
pub use marker::{
    MARKER_PREFIX, MARKER_SUFFIX, check_first_line, marker_for_version, rehead_actionlint_marker,
    validate_version, with_marker,
};
pub use rendered::{ACTIONLINT_PATH, RenderedFile, RenderedSymlink, RenderedTree};
pub use workflow_size::MAX_WORKFLOW_BYTES;
pub use yaml::{Yaml, quote_scalar, render_yaml};
