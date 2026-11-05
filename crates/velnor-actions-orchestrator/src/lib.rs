// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod crate_release_inventory_read;

macro_rules! private_mods {
    ($($module:ident),* $(,)?) => {
        $(mod $module;)*
    };
}

include!("private_modules.rs");
mod crate_release_archive;
pub mod crate_release_checkout;
pub mod crate_release_controller;
mod crate_release_git_bundle;
pub mod crate_release_github;
pub mod crate_release_http;
mod crate_release_pr_acquisition;
pub mod crate_release_pr_artifact;
mod crate_release_pr_run;
mod crate_release_pr_tree;
mod crate_release_pr_writer;
pub mod crate_release_qualification_approval;
pub mod crate_release_qualification_source;
pub mod crate_release_qualification_bundle;
pub mod crate_release_qualification_observation;
pub mod crate_release_qualification_witness;
pub mod crate_release_qualification_workflow;
pub mod crate_release_registry;
mod crate_release_runtime;
mod crate_release_semver;
pub mod crate_release_tool_artifact;
pub mod crate_release_tool_download;
pub mod github_review_feedback;
pub use crate_release_runtime::{parse_crate_release_intent, run_crate_release};
pub mod crate_release_git;
mod crate_release_source;
mod crate_release_workspace;
pub use crate_release_archive::{verify_archive_tracked_content, verify_crate_archive};
pub use crate_release_source::{
    CrateReleaseSourcePackage, journal_source_inventory, verified_rust_tool_path,
    verify_crate_release_source, verify_journal_source, verify_owned_tool_homes,
};
pub use crate_release_workspace::{
    CrateReleaseWorkspacePlan, crate_release_workspace_plan_with_registry,
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
