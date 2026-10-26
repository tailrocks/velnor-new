// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod affected_inputs;
mod api;
mod attach;
mod binary_preview_emit;
mod binary_release_emit;
mod build_task_tools;
mod build_tasks;
mod bun_assets_handoff;
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
mod github_authority_read;
mod github_landed_pull;
mod homebrew;
mod image_build_tasks;
mod image_maintenance;
mod image_prebuilt;
mod init;
mod internal;
mod internal_plan;
mod internal_request;
mod inventory;
mod inventory_reuse;
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
mod package_release_public;
mod package_release_publish;
mod package_release_signer;
mod parallel_target_seed;
mod pins;
mod plan;
mod plan_dispositions;
mod plan_output_limits;
mod plan_stacks;
mod prepare;
mod preseed_manifest;
mod preview_producer;
mod product_release_pins;
pub(crate) mod project_tool_sources;
mod provenance;
mod publish_emit;
mod publish_release_steps;

mod publish_steps;
mod qualification_candidate;
mod qualification_resolver;
mod qualify;
mod recommendations;
mod release_checkouts;
mod release_emit;
mod release_identity;
mod release_steps;
mod release_task_identity;
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
mod select_reads;
mod select_reads_lex;
mod select_reads_scan;
mod select_symlinks;
mod select_tofu;
mod source_cache;
mod source_prep;
mod stable_release_emit;
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
mod validators;
mod vectors;
mod verification_tasks;
mod verify;
mod workflow;
mod workflow_jobs;
mod workflow_task_jobs;

pub use api::*;

mod review_completion_workflow_emit;
