// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod audit_job;
mod base_cache;
mod build_profile;
mod build_script;
mod build_target;
mod cache_budget;
mod cargo_flags;
mod cargo_lock;
mod classify;
mod compat;
mod config;
mod config_events;
mod config_fork;
mod config_helper;
mod config_mirror;
mod config_pr_ci;
mod config_test_setup;
mod credentials;
mod detect;
mod doc_steps;
mod embedded;
mod error;
mod external_config;
mod harness;
mod lane;
mod lock;
mod lock_affected;
mod lock_key;
mod manifest_diff;
mod metadata;
mod metadata_file;
mod mise;
mod mise_lock;
mod os_pins;
mod owners;
mod plan;
mod policy;
mod private_sources;
mod reads;
mod repo_profile;
mod report;
mod required_job;
mod required_size;
mod root_jobs;
mod roots;
mod schedule;
mod select;
mod select_event;
mod sha256;
mod shape;
mod sibling_features;
mod target_dir;
mod test_setup;
mod timings;
mod toml_file;
mod toolchain;
mod unowned;
mod verdict;
mod weaken;
mod weaken_nextest;
mod weaken_scope;
mod weaken_targets;

pub use base_cache::{MAX_SOURCE_ROOTS, source_roots_over_limit};
pub use build_profile::{BuildProfile, build_profile, rustc_release};
pub use build_script::{SilentBuildScript, build_script_sources, silent_build_scripts};
pub use compat::{BUILD_DRIVER, COMPAT_SCHEMA, CompatInputs, compat};
pub use config::{CONFIG_PATH, ci_debuginfo};
pub use credentials::reject_release_credential_hooks;
pub use detect::{cargo_files, detect, manifests, metadata_roots, wanted_files};
pub use doc_steps::{member_sources, optional_steps, root_sources};
pub use embedded::is_plain_source;
pub use error::{DetectError, Reason};
pub use external_config::external_config_profile;
pub use lock::{
    LOCK_PATH, LockError, binary_sums_url, commit_url, lists_binary_sums, pin_lock,
    release_version, sums_url,
};
pub use lock_affected::lock_affected;
pub use lock_key::lock_key;
pub use metadata::{MetadataError, reduce_metadata};
pub use metadata_file::{
    METADATA_PATH, MetadataFileError, VERSIONS_HELPER, metadata_file, recorded_members,
    recorded_metadata, recorded_roots, recorded_target_dirs,
};
pub use mise::{MISE_CONFIGS, MISE_LOCK, check_tools, mise_files, tool_pins};
pub use owners::{Owner, Owners, add_reads};
pub use plan::{check_required_check, plan};
pub use policy::{PolicyRuns, policy_runs};
pub use reads::{
    INPUTS_LOCK, Located, dep_info_paths, format_inputs_lock, locate, normalize, parse_inputs_lock,
    rerun_if_changed, unlisted,
};
pub use repo_profile::{cargo_config_paths, check_runtime_configs, repo_profile};
pub use required_job::tests_expected;
pub use select::{select, unknown_paths, unread_packages};
pub use select_event::{select_event, selects_by_diff};
pub use sha256::sha256_hex;
pub use shape::{SECOND_JOB_COST_SECONDS, SHARD_THRESHOLD_SECONDS, decide_shape};
pub use timings::{TIMINGS_PATH, render_timings, root_job_ids};
pub use toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, toolchain};
pub use verdict::{
    JobResult, Need, PackageCounts, PlanOutput, TestCounts, Verdict, decide, encode_package_counts,
    package_token, verdict,
};
pub use weaken::introduced;

pub use cache_budget::{
    CacheEntry, CacheFamily, CacheId, DEFAULT_CACHE_BUDGET_BYTES, Timestamp, budget_plan,
    cache_family, delta_pull_request,
};
pub use private_sources::{cacheable_config, cacheable_lock};
