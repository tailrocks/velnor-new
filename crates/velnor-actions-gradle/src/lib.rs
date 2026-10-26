// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
pub mod analysis;
pub mod catalog;
pub mod edges;
pub mod integrity;
pub mod lex;
pub mod script;
pub mod settings;
pub mod version;
pub mod wrapper;

pub const STACK_ID: &str = "gradle";
