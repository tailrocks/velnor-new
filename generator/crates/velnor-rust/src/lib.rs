// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod config;
mod detect;
mod lock;
mod metadata;
mod plan;

pub use config::CONFIG_PATH;
pub use detect::{DetectError, detect, manifests, metadata_roots, wanted_files};
pub use lock::{LOCK_PATH, LockError, pin_lock, release_version, sums_url};
pub use metadata::{MetadataError, reduce_metadata};
pub use plan::plan;
