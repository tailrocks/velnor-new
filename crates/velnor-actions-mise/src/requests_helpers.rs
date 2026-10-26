// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::mise_argv_tail;

pub(super) fn full_mise_argv(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    payload: &[OsString],
) -> Vec<OsString> {
    let specs = catalog.tool_specs(tools);
    let mut argv = vec![OsString::from("mise")];
    argv.extend(mise_argv_tail("exec", &specs, payload));
    argv
}
