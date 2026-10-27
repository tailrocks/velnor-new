// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "check_tool_acquire/acquire.rs"]
mod acquire;
#[path = "check_tool_acquire/observe.rs"]
mod observe;
#[path = "check_tool_acquire/shared.rs"]
mod shared;

#[path = "check_tool_archive.rs"]
pub(crate) mod archive;
#[path = "check_tool_layout.rs"]
pub(crate) mod layout;
#[path = "check_tool_tree.rs"]
pub(crate) mod tree;

pub(super) use acquire::acquire;
pub(crate) use observe::{
    check_deadline, executable_sha256, read_with_deadline, sha256_with_deadline,
    validate_installations,
};
