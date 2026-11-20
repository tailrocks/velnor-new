// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub use crate::base_cache::{MAX_SOURCE_ROOTS, source_roots_over_limit};
pub use crate::build_profile::{BuildProfile, build_profile, rustc_release};
pub use crate::build_script::{SilentBuildScript, build_script_sources, silent_build_scripts};
pub use crate::cache_budget::{
    CacheEntry, CacheFamily, CacheId, DEFAULT_CACHE_BUDGET_BYTES, Timestamp, budget_plan,
    cache_family, delta_pull_request, target_family_prefix,
};
pub use crate::classify::is_workflow_file;
pub use crate::compat::{
    BUILD_DRIVER, COMPAT_SCHEMA, CompatInputs, WRAPPED_BUILD_DRIVER, build_driver, compat,
};
pub use crate::config::{CONFIG_PATH, budget, ci_debuginfo};
pub use crate::config_keys::KEYS;
pub use crate::credentials::reject_release_credential_hooks;
pub use crate::detect::{cargo_files, detect, manifests, metadata_roots, wanted_files};
pub use crate::doc_steps::{member_sources, optional_steps, root_sources};
pub use crate::docs_post_merge::defer_post_merge_docs;
pub use crate::embedded::{is_plain_source, is_rust_source};
pub use crate::error::{DetectError, Reason};
pub use crate::excluded_lock::{ExcludedLockFinding, excluded_lock_findings};
pub use crate::external_config::external_config_profile;
pub use crate::group_runs::{GroupPlan, GroupRun, group_runs, plan_groups};
pub use crate::lint_error::LintFault;
pub use crate::lock::{
    LOCK_PATH, LockError, binary_sums_url, commit_url, lists_binary_sums, pin_lock,
    release_version, sums_url,
};
pub use crate::lock_affected::lock_affected;
pub use crate::lock_key::lock_key;
pub use crate::metadata::{MetadataError, reduce_metadata};
pub use crate::metadata_file::{
    METADATA_PATH, MetadataFileError, metadata_file, recorded_members, recorded_metadata,
    recorded_roots, recorded_target_dirs,
};
pub use crate::mise::{MISE_CONFIGS, MISE_LOCK, check_tools, mise_files, tool_pins};
pub use crate::owners::{Owner, Owners, add_reads};
pub use crate::plan::{check_required_check, plan, plan_without_deltas};
pub use crate::policy::{PolicyRuns, policy_runs};
pub use crate::private_sources::cacheable_config;
pub use crate::public_git::{LockAdmission, lock_admission};
pub use crate::reads::{
    INPUTS_IDENTITY_HELPER, INPUTS_LOCK, Located, dep_info_paths, format_inputs_lock,
    inputs_lock_text, locate, lock_is_identified, normalize, parse_inputs_lock, rerun_if_changed,
    resolve, unlisted, without_identity,
};
pub use crate::repo_profile::{cargo_config_paths, check_runtime_configs, repo_profile};
pub use crate::required_job::tests_expected;
pub use crate::select::{select, unknown_paths, unread_packages};
pub use crate::select_event::{select_event, selects_by_diff};
pub use crate::sha256::sha256_hex;
pub use crate::shape::{
    SECOND_JOB_COST_SECONDS, SHARD_THRESHOLD_SECONDS, decide_shape, sparse_checkout_planned,
    untimed_auto_note,
};
pub use crate::sparse::bytes_outside_lanes;
pub use crate::suggest_groups::suggest_groups;
pub use crate::target_dir::{NESTED_ROOTS_FOLDER, default_folder};
pub use crate::timings::{TIMINGS_PATH, render_timings, root_job_ids};
pub use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, toolchain};
pub use crate::verdict::{
    JobResult, PackageCounts, TestCounts, Verdict, decide, encode_package_counts, package_token,
    verdict,
};
pub use crate::verdict_plan::{Need, PlanOutput};
pub use crate::weaken::introduced;
