// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::EncodedJit;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunnerReference {
    pub id: i64,

    pub name: String,

    pub runner_scale_set_id: i64,
}

#[derive(Debug)]
pub struct JitResult {
    pub encoded_jit_config: EncodedJit,

    pub runner: RunnerReference,
}
