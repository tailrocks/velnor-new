//! Generation coordination: discovery, planning, workflow IR, and writes.
//! Execution belongs to Mise, vector requests to vectors, and YAML to the renderer.

mod affected_inputs;
mod api;
mod attach;
mod baseline_publish;
mod binary_preview_emit;
mod binary_release_emit;
mod build_task_tools;
mod build_tasks;
pub mod cache_action_stage;
mod cache_keys;
mod check_evidence;
mod check_runtime;
mod clippy_groups;
mod config;
mod config_stacks;
mod cover;
mod crate_job_ids;
mod crate_jobs;
mod critical_path;
pub mod decisions;
mod derive_groups;
mod discover;
mod discover_index;
mod discover_tofu;
mod error;
mod evidence;
mod exclusive_write;
mod extension_schemas;
mod filesystem_owner;
mod finalized;
mod freshness_emit;
mod generate;
mod generator;
mod gh_download;
mod git_paths;
mod init;
mod internal;
mod internal_plan;
mod internal_request;
mod inventory;
mod inventory_reuse;
mod job_authority_inventory;
mod job_authority_inventory_types;
mod job_image_preview;
pub mod local_release_manifest;
mod lock_audit;
mod matrix_step;
mod mbx_preflight;
mod merge;
mod merge_request;
mod native_image_tasks;
mod native_mise_tasks;
mod native_mise_version;
mod native_tool_input;
mod native_tool_lock;
mod noop_report;
mod obligation_order;
mod origin;
mod owned_tool_preview;
mod owned_tool_publication;
mod package_publication;
mod package_release_artifacts;
mod package_release_publish;
mod package_release_signer;
mod pins;
mod plan;
mod plan_dispositions;
mod plan_output_limits;
mod plan_stacks;
mod prepare;
mod preseed_manifest;
mod preview_producer;
mod product_release_pins;
mod provenance;
mod publish_job;
mod qualification_cache_backend;
mod qualification_cache_payload;
mod qualification_cache_runtime;
mod qualification_candidate;
mod qualification_resolver;
mod qualification_source_evidence;
mod qualify;
mod recommendations;
mod release_checkouts;
mod release_emit;
mod release_identity;
mod release_steps;
mod request_event;
mod required_dispositions;
mod retrieve_reports;
mod retrieve_retry;
mod root;
mod routing;
pub(crate) mod rust_suites_emit;
mod safe_read;
pub mod schedule;
mod select;
mod select_affected;
mod select_edges;
mod select_symlinks;
mod select_tofu;
mod source_cache;
mod source_prep;
mod tap_merge;
mod tap_prepare;
mod tap_source_freshness;
mod task_execution;
pub(crate) mod task_report;
mod task_report_aggregate;
mod tofu_apply_emit;
mod tofu_cache;
pub(crate) mod tool_catalog_project;
pub(crate) mod tool_catalog_resolve;
mod toolcheck;
mod toolfindings;
mod utf8;
mod validate;
mod validate_shell;
mod validate_zizmor;
mod validators;
mod vectors;
mod verification_tasks;
mod verify;
mod workflow;
mod workflow_jobs;
mod workflow_jobs_cache;
mod workflow_task_jobs;

#[cfg(all(feature = "test-render-capture", test))]
#[path = "test_render_capture_harness.rs"]
mod test_render_capture_harness;

#[cfg(test)]
pub(crate) mod tap_test_support;

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

pub use api::*;
pub use qualification_candidate::qualify_tools;

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

mod review_completion_workflow_emit;
