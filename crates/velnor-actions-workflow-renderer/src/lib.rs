//! Typed-IR to YAML rendering for CI, release, and freshness workflows.
//!
//! Validated IR plus fixed argv in, marked YAML out: no subprocesses, no
//! stack or tool branching, quoting-only shell shaping.

mod action_ref;
pub mod agents_md;
mod artifact_paths;
pub mod cache_elect;
pub mod cache_p08;
mod cache_p08_detect;
mod cache_steps;
mod candidate;
pub mod closure;
mod closure_paths;
mod commands;
mod commands_env;
mod commands_scan;
mod composite;
mod dispatch_cache_boundary;
mod document;
mod document_env;
mod document_lanes;
mod document_steps;
mod error;
mod expressions;
mod final_steps;
pub mod freshness;
pub mod guard;
mod lane_share;
mod lane_share_sections;
pub mod lane_target;
pub mod marker;
mod matrix;
mod matrix_output_mode;
mod mbx_gc_policy;
pub mod msrv;
pub mod overlap;
pub mod owned_tool_publication;
pub mod plan_format;
pub mod preseed;
mod preseed_closure;
pub mod release_checkout_gates;
pub mod release_config;
pub mod release_gates;
pub mod release_jobs;
pub mod release_permissions;
pub mod release_spec;
pub mod release_tree;
pub mod render;
mod render_cache_files;
mod runs_on;
pub mod schema2;
pub mod setup;
mod step_ids;
pub mod steps;
mod steps_artifact;
mod steps_internal;
mod steps_plain;
mod steps_shell;
mod support;
pub mod tofu_apply;
mod tofu_apply_command;
mod tofu_apply_document;
mod tofu_apply_policy;
mod tofu_apply_steps;
pub mod tofu_cache;
mod tool_seed;
mod tool_seed_admission;
#[cfg(test)]
mod tool_seed_test_support;
pub mod toolchain_env;
pub mod tree;
pub mod verification_jobs;
mod workflow_policy;
mod workflow_size;
pub mod yaml;

mod root_api;
pub use self::root_api::*;

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;
