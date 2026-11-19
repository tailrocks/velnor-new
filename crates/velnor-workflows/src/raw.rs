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
    pub(crate) hosted: Option<bool>,
    pub(crate) fork_runner: Option<String>,
    pub(crate) hosted_runner: Option<String>,
    pub(crate) scale_set_runner: Option<String>,
    #[serde(rename = "if")]
    pub(crate) condition: Option<String>,
    pub(crate) concurrency: Option<RawConcurrency>,
    pub(crate) on: Option<RawOn>,
    #[serde(default)]
    pub(crate) input: Vec<RawInput>,
    #[serde(default)]
    pub(crate) output: Vec<RawOutput>,
    #[serde(default)]
    pub(crate) job: Vec<RawJob>,
    #[serde(default)]
    pub(crate) case: Vec<RawCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCase {
    pub(crate) id: String,
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) modes: Vec<String>,
    #[serde(rename = "if")]
    pub(crate) condition: Option<String>,
    pub(crate) timeout_minutes: Option<u16>,
    #[serde(default)]
    pub(crate) permissions: BTreeMap<String, String>,
    pub(crate) container: Option<RawContainer>,
    #[serde(default)]
    pub(crate) service: Vec<RawService>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) cache_mode: Option<String>,
    #[serde(default)]
    pub(crate) step: Vec<RawStep>,
}

/// The `[workflow.on]` table. An absent table means `workflow_call` alone.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOn {
    pub(crate) workflow_call: Option<bool>,
    pub(crate) workflow_dispatch: Option<RawDispatch>,
    #[serde(default)]
    pub(crate) schedule: Vec<String>,
    pub(crate) push: Option<RawPush>,
    pub(crate) pull_request: Option<RawPullRequest>,
    pub(crate) merge_group: Option<bool>,
    pub(crate) workflow_run: Option<RawWorkflowRun>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDispatch {
    #[serde(default)]
    pub(crate) input: Vec<RawInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPush {
    #[serde(default)]
    pub(crate) branches: Vec<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) paths: Vec<String>,
    #[serde(default)]
    pub(crate) paths_ignore: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPullRequest {
    #[serde(default)]
    pub(crate) types: Vec<String>,
    #[serde(default)]
    pub(crate) branches: Vec<String>,
    #[serde(default)]
    pub(crate) paths: Vec<String>,
    #[serde(default)]
    pub(crate) paths_ignore: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConcurrency {
    pub(crate) group: String,
    #[serde(default)]
    pub(crate) cancel_in_progress: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawWorkflowRun {
    #[serde(default)]
    pub(crate) workflows: Vec<String>,
    #[serde(default)]
    pub(crate) types: Vec<String>,
    #[serde(default)]
    pub(crate) branches: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) required: Option<bool>,
    #[serde(rename = "type")]
    pub(crate) kind: Option<String>,
    pub(crate) default: Option<toml::Value>,
    #[serde(default)]
    pub(crate) options: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOutput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawJob {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) needs: Vec<String>,
    #[serde(rename = "if")]
    pub(crate) condition: Option<String>,
    #[serde(default)]
    pub(crate) helper: bool,
    pub(crate) runner: Option<String>,
    pub(crate) hosted: Option<bool>,
    pub(crate) environment: Option<String>,
    pub(crate) timeout_minutes: Option<u16>,
    pub(crate) concurrency: Option<RawConcurrency>,
    #[serde(default)]
    pub(crate) permissions: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) cache_mode: Option<String>,
    #[serde(default)]
    pub(crate) outputs: BTreeMap<String, String>,
    pub(crate) matrix: Option<RawMatrix>,
    pub(crate) container: Option<RawContainer>,
    #[serde(default)]
    pub(crate) service: Vec<RawService>,
    pub(crate) uses: Option<String>,
    #[serde(default)]
    pub(crate) with: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub(crate) step: Vec<RawStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMatrix {
    pub(crate) from_output: Option<String>,
    pub(crate) axes: Option<BTreeMap<String, Vec<String>>>,
    pub(crate) fail_fast: Option<bool>,
    pub(crate) max_parallel: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStep {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    #[serde(rename = "if")]
    pub(crate) condition: Option<String>,
    pub(crate) run: Option<String>,
    pub(crate) uses: Option<String>,
    pub(crate) helper: Option<String>,
    #[serde(default)]
    pub(crate) with: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) continue_on_error: bool,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawContainer {
    pub(crate) image: String,
    pub(crate) options: Option<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawService {
    pub(crate) name: String,
    pub(crate) image: String,
    #[serde(default)]
    pub(crate) ports: Vec<String>,
    pub(crate) options: Option<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}
