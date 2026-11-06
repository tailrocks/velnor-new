// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#[cfg(test)]
extern crate self as velnor_actions_orchestrator;
mod crate_release_inventory_read;

mod affected_inputs;
mod api;
mod attach;
mod binary_preview_emit;
mod binary_release_emit;
mod build_task_tools;
mod build_tasks;
mod bun_assets_handoff;
mod cache_admission;
mod cache_archive;
mod cache_inventory;
mod cache_snapshot;
mod check_evidence;
mod check_runtime;
mod clippy_groups;
mod config;
mod config_stacks;
mod controller_approval;
mod controller_manifest_digest;
mod cover;
mod crate_job_ids;
mod crate_jobs;
mod crate_release_archive;
mod crate_release_artifact_pages;
pub mod crate_release_checkout;
pub mod crate_release_controller;
#[cfg(target_os = "linux")]
mod crate_release_custody;
mod crate_release_git_bundle;
pub mod crate_release_github;
#[cfg(target_os = "linux")]
mod crate_release_historical_intent;
pub mod crate_release_http;
#[cfg(target_os = "linux")]
mod crate_release_pinned_download;
mod crate_release_pr_acquisition;
pub mod crate_release_pr_artifact;
mod crate_release_pr_run;
mod crate_release_pr_tree;
mod crate_release_pr_writer;
#[cfg(target_os = "linux")]
mod crate_release_projected_tools;
pub mod crate_release_qualification_approval;
pub mod crate_release_qualification_bundle;
pub mod crate_release_qualification_capture;
pub mod crate_release_qualification_checks;
mod crate_release_qualification_deletions;
pub mod crate_release_qualification_inputs;
pub mod crate_release_qualification_intent;
pub mod crate_release_qualification_manifest;
pub mod crate_release_qualification_observation;
pub mod crate_release_qualification_package;
pub mod crate_release_qualification_provider;
pub mod crate_release_qualification_snapshot;
pub mod crate_release_qualification_source;
pub mod crate_release_qualification_witness;
pub mod crate_release_qualification_workflow;
pub mod crate_release_registry;
mod crate_release_runtime;
mod crate_release_semver;
#[cfg(target_os = "linux")]
mod crate_release_source_view;
#[cfg(target_os = "linux")]
mod crate_release_source_wire;
pub mod crate_release_tool_artifact;
pub mod crate_release_tool_download;
mod critical_path;
mod derive_groups;
mod discover;
mod discover_index;
mod discover_tofu;
mod error;
mod evidence;
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
pub mod github_review_feedback;
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
mod native_validator_tool_qualification;
mod non_inputs;
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
mod required_dispositions;
mod retrieve_reports;
mod retrieve_retry;
mod review_completion_workflow_emit;
mod root;
mod routing;
mod safe_read;
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
mod state_roots;
mod tap_history_relevance;
mod tap_merge;
mod tap_prepare;
mod tap_source_freshness;
mod tap_writer_history_timestamp;
mod task_execution;
mod task_report_aggregate;
mod tofu_apply_emit;
mod tofu_cache;
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
pub use crate_release_runtime::{parse_crate_release_intent, run_crate_release};

pub fn render_crate_publisher_entry(timeout_seconds: u32) -> Result<String, OrchestratorError> {
    Ok(velnor_actions_workflow_renderer::release_root_publisher::render_entry(timeout_seconds)?)
}

pub mod crate_release_git;
mod crate_release_source;
mod crate_release_workspace;
pub use crate_release_archive::{verify_archive_tracked_content, verify_crate_archive};
pub use crate_release_source::{
    CrateReleaseSourcePackage, journal_source_inventory, verified_rust_tool_path,
    verify_crate_release_source, verify_journal_source, verify_owned_tool_homes,
};
pub use crate_release_workspace::{
    CratePackageSource, CrateReleaseWorkspacePlan, CrateSemverSource, CrateSource,
    CrateSourceExecution, HistoricalIntentReader, UnprivilegedSourceExecution,
    crate_release_workspace_plan_with_registry,
};
#[expect(
    missing_debug_implementations,
    reason = "T52 binds the T26-owned publication interface; the Debug derive stays with the owning task"
)]
pub mod cache_publication;
#[expect(
    missing_debug_implementations,
    reason = "T52 binds the T26-owned recovery interface; the Debug derive stays with the owning task"
)]
pub mod cache_recovery;
pub mod decisions;
#[cfg(test)]
mod git_fixture;
pub mod local_release_manifest;
pub(crate) mod project_tool_sources;
pub(crate) mod rust_suites_emit;
pub mod schedule;
pub(crate) mod task_report;
pub(crate) mod tool_catalog_project;
pub(crate) mod tool_catalog_resolve;

pub use api::*;

pub mod crate_registry_inventory;
pub use crate_registry_inventory::{
    CrateInventoryError, CrateRegistryInventory, CrateRegistryVersion, inspect_crate_versions,
};
pub mod crate_release_journal;
pub mod crate_release_journal_adapter;
mod crate_release_journal_github;
pub mod fd_tool_extract;

#[cfg(test)]
#[path = "../tests/crate_archive_public.rs"]
mod crate_archive_public;
#[cfg(test)]
#[path = "../tests/crate_release_pr_artifact_public.rs"]
mod crate_release_pr_artifact_public;
#[cfg(test)]
#[path = "../tests/crate_source_public.rs"]
mod crate_source_public;
#[cfg(test)]
#[path = "../tests/crate_workspace_plan_public.rs"]
mod crate_workspace_plan_public;

#[cfg(test)]
mod offline_attestation_test;
