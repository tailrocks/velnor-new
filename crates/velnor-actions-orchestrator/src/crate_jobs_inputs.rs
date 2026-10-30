// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::SelectedTestTool;
use velnor_actions_contract::{RustCrateConfig, Step, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;

use crate::discover::Discovery;

#[derive(Clone, Copy)]
pub(crate) struct CrateJobInputs<'a> {
    pub(crate) label: &'a str,
    pub(crate) policy: WorkflowPolicy,
    pub(crate) discovery: &'a Discovery,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) fetch_roots: &'a [String],
    pub(crate) acquire: Option<&'a Step>,
    pub(crate) max_parallel_jobs: u32,
    pub(crate) timeout: velnor_actions_contract::JobTimeout,
    pub(crate) helper_version: &'a str,
    pub(crate) seed_parallel_targets: bool,
    pub(crate) crates: &'a BTreeMap<String, RustCrateConfig>,
    pub(crate) test_tools: &'a BTreeMap<String, Vec<SelectedTestTool>>,
}
