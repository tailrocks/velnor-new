// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFile {
    #[serde(default)]
    pub(crate) workflow: Vec<RawWorkflow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawWorkflow {
    pub(crate) file: String,
    pub(crate) name: String,
    pub(crate) runner: Option<String>,
    #[serde(default)]
    pub(crate) input: Vec<RawInput>,
    #[serde(default)]
    pub(crate) job: Vec<RawJob>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) required: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawJob {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) needs: Vec<String>,
    #[serde(default)]
    pub(crate) helper: bool,
    pub(crate) timeout_minutes: Option<u16>,
    #[serde(default)]
    pub(crate) permissions: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) outputs: BTreeMap<String, String>,
    pub(crate) matrix: Option<RawMatrix>,
    #[serde(default)]
    pub(crate) step: Vec<RawStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMatrix {
    pub(crate) from_output: String,
    pub(crate) fail_fast: Option<bool>,
    pub(crate) max_parallel: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStep {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) run: String,
    #[serde(default)]
    pub(crate) continue_on_error: bool,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}
