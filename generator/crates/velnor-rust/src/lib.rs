// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod config;
mod detect;
mod plan;

pub use config::CONFIG_PATH;
pub use detect::{DetectError, detect};
pub use plan::plan;
