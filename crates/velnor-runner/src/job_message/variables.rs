// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::Deserialize;

use super::wire::deserialize_null_default;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableValue {
    #[serde(default, alias = "Value")]
    pub value: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        alias = "IsSecret"
    )]
    pub is_secret: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskHint {
    #[serde(default, rename = "type", alias = "Type")]
    pub kind: Option<String>,
    #[serde(default, alias = "Value")]
    pub value: Option<String>,
}
