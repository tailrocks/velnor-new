// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod build_profile;
mod cargo_lock;
mod classify;
mod compat;
mod config;
mod detect;
mod doc_steps;
mod error;
mod lock;
mod lock_affected;
mod manifest_diff;
mod metadata;
mod metadata_file;
mod mise;
mod owners;
mod plan;
mod reads;
mod roots;
mod select;
mod sha256;
mod shape;
mod timings;
mod toml_file;
mod toolchain;
mod unowned;

pub use build_profile::{BuildProfile, build_profile};
pub use compat::{BUILD_DRIVER, COMPAT_SCHEMA, CompatInputs, compat};
pub use config::{CONFIG_PATH, ci_debuginfo};
pub use detect::{cargo_files, detect, manifests, metadata_roots, wanted_files};
pub use doc_steps::{optional_steps, root_sources};
pub use error::{DetectError, Reason};
pub use lock::{LOCK_PATH, LockError, commit_url, pin_lock, release_version, sums_url};
pub use lock_affected::lock_affected;
pub use metadata::{MetadataError, reduce_metadata};
pub use metadata_file::{METADATA_PATH, MetadataFileError, metadata_file, recorded_metadata};
pub use mise::{check_tools, mise_files, tool_pins};
pub use owners::{Owner, Owners, add_reads};
pub use plan::{check_required_check, plan};
pub use reads::{
    INPUTS_LOCK, Located, dep_info_paths, format_inputs_lock, locate, normalize, parse_inputs_lock,
    rerun_if_changed, unlisted,
};
pub use select::{select, unknown_paths, unread_packages};
pub use sha256::sha256_hex;
pub use shape::{SECOND_JOB_COST_SECONDS, SHARD_THRESHOLD_SECONDS, decide_shape};
pub use timings::TIMINGS_PATH;
