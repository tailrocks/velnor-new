// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod config;
mod detect;
mod metadata;
mod plan;

pub use config::CONFIG_PATH;
pub use detect::{DetectError, detect, metadata_roots, wanted_files};
pub use metadata::{MetadataError, reduce_metadata};
pub use plan::plan;
