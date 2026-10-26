// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod capacity;
mod error;
mod load;
mod memory;
mod pressure;
mod record;
mod sample;
mod units;

pub use error::ProbeError;
pub use record::{MAX_OUTPUT_BYTES, ProbeRecord};
pub use sample::sample;
