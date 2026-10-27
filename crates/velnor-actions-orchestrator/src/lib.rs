// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
macro_rules! private_mods {
    ($($module:ident),* $(,)?) => {
        $(mod $module;)*
    };
}

private_mods!(affected_inputs, api, attach);
private_mods!(binary_preview_emit, binary_release_emit);
private_mods!(build_task_tools, build_tasks);
private_mods!(bun_assets_handoff, cache_admission);
private_mods!(cache_archive, cache_inventory);
private_mods!(cache_snapshot);
private_mods!(check_evidence, check_runtime);
private_mods!(clippy_groups, config);
private_mods!(config_stacks, cover);
private_mods!(crate_job_ids, crate_jobs);
private_mods!(critical_path, derive_groups);
private_mods!(discover, discover_index);
private_mods!(discover_tofu, error, evidence);
private_mods!(exclusive_write, extension_schemas);
private_mods!(filesystem_owner, finalized);
private_mods!(freshness_emit, generate);
private_mods!(generator, gh_download, git_paths);
#[cfg(test)]
mod git_fixture;
private_mods!(github_authority_read, github_landed_pull, homebrew);
private_mods!(image_build_tasks, image_maintenance);
private_mods!(image_prebuilt, init, internal);
private_mods!(internal_plan, internal_request);
private_mods!(inventory, inventory_reuse);
private_mods!(lock_audit, matrix_step);
private_mods!(mbx_preflight, merge);
private_mods!(merge_request, native_image_tasks);
private_mods!(native_mise_tasks, native_mise_version);
private_mods!(native_tool_input, native_tool_lock);
private_mods!(noop_report, obligation_order);
private_mods!(origin, owned_tool_preview);
private_mods!(owned_tool_publication);
private_mods!(package_publication);
private_mods!(package_release_artifacts);
private_mods!(package_release_public);
private_mods!(package_release_publish);
private_mods!(package_release_signer);
private_mods!(parallel_target_seed);
private_mods!(pins, plan, plan_dispositions);
private_mods!(plan_output_limits, plan_stacks);
private_mods!(prepare, preseed_manifest);
private_mods!(preview_producer, product_release_pins);
private_mods!(provenance, publish_emit);
private_mods!(publish_release_steps, publish_steps);
private_mods!(qualification_candidate);
private_mods!(qualification_resolver);
private_mods!(qualify, recommendations);
private_mods!(release_checkouts, release_emit);
private_mods!(release_identity, release_steps);
private_mods!(release_task_identity);
private_mods!(request_event, required_dispositions);
private_mods!(retrieve_reports, retrieve_retry);
private_mods!(review_completion_workflow_emit);
private_mods!(root, routing, safe_read);
private_mods!(select, select_affected);
private_mods!(select_edges, select_reads);
private_mods!(select_reads_lex, select_reads_scan);
private_mods!(select_symlinks, select_tofu);
private_mods!(source_cache, source_prep);
private_mods!(stable_release_emit, tap_history_relevance, tap_merge);
private_mods!(
    tap_prepare,
    tap_source_freshness,
    tap_writer_history_timestamp
);
private_mods!(task_execution, task_report_aggregate);
private_mods!(tofu_apply_emit, tofu_cache);
private_mods!(toolcheck, toolfindings, utf8);
private_mods!(validate, validate_shell);
private_mods!(validators, vectors);
private_mods!(verification_tasks, verify);
private_mods!(workflow, workflow_jobs);
private_mods!(workflow_task_jobs);
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
pub mod local_release_manifest;
pub(crate) mod project_tool_sources;
pub(crate) mod rust_suites_emit;
pub mod schedule;
pub(crate) mod task_report;
pub(crate) mod tool_catalog_project;
pub(crate) mod tool_catalog_resolve;

pub use api::*;
