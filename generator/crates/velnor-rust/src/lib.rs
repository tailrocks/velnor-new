// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cargo_lock;
mod classify;
mod config;
mod detect;
mod doc_steps;
mod error;
mod lock;
mod lock_affected;
mod manifest_diff;
mod metadata;
mod mise;
mod plan;
mod reads;
mod roots;
mod select;
mod shape;
mod timings;
mod toml_file;
mod toolchain;

pub use config::CONFIG_PATH;
pub use detect::{cargo_files, detect, metadata_roots, wanted_files};
pub use doc_steps::{optional_steps, root_sources};
pub use error::{DetectError, Reason};
pub use lock::{LOCK_PATH, LockError, pin_lock, release_version, sums_url};
pub use lock_affected::lock_affected;
pub use metadata::{MetadataError, reduce_metadata};
pub use mise::{nextest_tool, toolchain_conflict};
pub use plan::plan;
pub use reads::{
    INPUTS_LOCK, Located, Owner, Owners, add_reads, dep_info_paths, format_inputs_lock, locate,
    normalize, parse_inputs_lock, rerun_if_changed, unlisted,
};
pub use select::{root_slug, select, unknown_paths};
pub use shape::{SECOND_JOB_COST_SECONDS, SHARD_THRESHOLD_SECONDS, decide_shape};
pub use timings::TIMINGS_PATH;
