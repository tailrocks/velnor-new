//! Typed-IR to YAML rendering for CI, release, and freshness workflows.
//!
//! Validated IR plus fixed argv in, marked YAML out: no subprocesses, no
//! stack or tool branching, quoting-only shell shaping.

pub mod render;
pub mod tree;

pub use render::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET, COVERED_TASKS_OUTPUT,
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, MatrixSource, PLAN_ID_OUTPUT,
    PLAN_STEP_ID, RUN_KEY_OUTPUT, WORKFLOW_PATH, render_workflow_ir, render_workflow_ir_strict,
};
pub use tree::{render_tree, render_tree_with_extra};

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;
