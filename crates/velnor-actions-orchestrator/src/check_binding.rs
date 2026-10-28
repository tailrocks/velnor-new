// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use velnor_actions_contract::{MatrixEntry, Plan};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, ToolCatalog};

use crate::check_runtime::ApprovedMiseRuntime;

#[derive(Clone, Copy)]
pub(super) struct CheckBinding<'a> {
    pub(super) root: &'a Path,
    pub(super) item: &'a DiscoveredCheck,
    pub(super) plan: &'a Plan,
    pub(super) entry: &'a MatrixEntry,
    pub(super) task_id: &'a str,
    pub(super) catalog: &'a ToolCatalog,
    pub(super) runtime: &'a ApprovedMiseRuntime,
    pub(super) deadline: CheckDeadline,
}
