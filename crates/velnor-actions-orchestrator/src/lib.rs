// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#[cfg(test)]
extern crate self as velnor_actions_orchestrator;
#[cfg(test)]
mod libtest_case;

mod api;
mod bun_assets_handoff;
mod cache_admission;
mod cache_archive;
mod cache_inventory;
mod cache_snapshot;
mod config;
mod controller_approval;
mod controller_manifest_digest;
pub mod crate_release_checkout;
pub mod crate_release_controller;
pub mod crate_release_github;
pub mod crate_release_http;
pub mod crate_release_pr_artifact;
pub mod crate_release_qualification_approval;
pub mod crate_release_qualification_bundle;
pub mod crate_release_qualification_capture;
pub mod crate_release_qualification_checks;
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
pub mod crate_release_tool_artifact;
pub mod crate_release_tool_download;
mod error;
mod filesystem_owner;
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
mod lock_audit;
mod mbx_preflight;
mod package_publication;
mod planner;
mod preview_producer;
mod release;
mod required_dispositions;
mod review_completion_workflow_emit;
mod safe_read;
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
pub use release::crate_release_runtime::{parse_crate_release_intent, run_crate_release};

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
pub mod crate_release_git;
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
