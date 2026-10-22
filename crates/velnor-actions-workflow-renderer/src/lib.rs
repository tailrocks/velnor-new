//! Typed-IR to YAML rendering for CI, release, and freshness workflows.
//!
//! Validated IR plus fixed argv in, marked YAML out. No subprocesses.

mod action_mbx_shell;
mod action_ref;
pub mod agents_md;
mod artifact_paths;
pub mod cache_cleanup;
mod cache_doctor;
pub mod cache_elect;
pub mod cache_p08;
mod cache_p08_detect;
mod cache_plan_outputs;
pub mod cache_prune;
mod cache_save_budget;
mod cache_steps;
mod candidate;
pub mod closure;
mod closure_paths;
mod commands;
mod commands_env;
mod commands_scan;
mod composite;
mod crate_obligations;
pub mod dco;
mod dispatch_cache_boundary;
mod document;
mod document_env;
mod document_evidence;
mod document_lanes;
mod document_services;
mod document_steps;
mod error;
mod expressions;
mod final_steps;
pub mod freshness;
pub mod guard;
pub mod image_publish_scripts;
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
mod parallel_steps;
mod parallel_target_seed;
pub mod plan_format;
mod plan_gate;
pub mod preseed;
mod preseed_closure;
pub mod private_source_steps;
/// Project tool-version resolver binding (hard decision 5).
mod project_tool_resolver;
pub mod publish_render;
pub mod publish_roles;
pub mod publish_scripts;
pub mod publish_spec;
pub mod release_checkout_gates;
pub mod release_config;
pub mod release_gates;
pub mod release_jobs;
pub mod release_permissions;
pub mod release_spec;
pub mod release_tree;
pub mod render;
mod render_cache_files;
mod required_gate;
mod runs_on;
pub mod rust_suites;
pub mod schema2;
pub mod setup;
mod step_ids;
pub mod steps;
mod steps_artifact;
mod steps_internal;
mod steps_plain;
mod steps_shell;
mod support;
mod task_wrapper;
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
pub mod upgrades;

/// Display name of the step that verifies an installed Rust toolchain file
/// by file against its pinned channel manifest; every generated job that
/// installs Rust runs it before the toolchain is used.
pub const VERIFY_RUST_TOOLCHAIN_STEP: &str = "Verify Rust channel manifest";

/// The only `PATH` a cache or toolchain verification script runs with:
/// root-owned system directories. Setup Mise has already put the Mise
/// `bin/` and `shims/` directories on `PATH` ahead of them, and the tools
/// cache restores both, so a bare helper name (`sha256sum`, `curl`, `tar`)
/// resolved through the inherited `PATH` could run cached code inside the
/// very check meant to gate it. Each script assigns this first.
pub const VERIFY_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Shell of the composite steps that run while restored, unverified bytes
/// sit on `PATH`. The runner resolves a bare `bash` through `PATH`
/// (`WhichUtil.Which`, with the `addPath` entries first) but runs a fully
/// qualified path as given.
pub const VERIFY_SHELL: &str = "/bin/bash --noprofile --norc -e -o pipefail {0}";
pub mod verification_jobs;
pub mod verifier_composite;
mod workflow_policy;
mod workflow_size;
mod workflow_timeout;
pub mod yaml;

mod root_api;
pub use self::root_api::*;

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;

/// Source-free authenticated review completion companion.
pub mod review_completion;
