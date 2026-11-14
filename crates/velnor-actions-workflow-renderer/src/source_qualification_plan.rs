// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceQualificationPlan {
    pub(crate) job: crate::yaml::Yaml,
    pub(crate) cli_routes: Vec<String>,
    pub(crate) non_inputs: velnor_actions_contract::NonInputs,
    pub(crate) controller_path: String,
    pub(crate) controller_version: String,
}
