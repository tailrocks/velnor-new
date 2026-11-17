// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::JobId;
use velnor_rust::Need;

use crate::error::CiError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNeed {
    result: Option<String>,
    #[serde(default)]
    outputs: BTreeMap<String, String>,
}

pub fn parse_needs(text: &str) -> Result<BTreeMap<JobId, Need>, CiError> {
    let raw: BTreeMap<String, RawNeed> = serde_json::from_str(text)
        .map_err(|e| CiError::new(format!("NEEDS_JSON is not the needs context: {e}")))?;
    raw.into_iter()
        .map(|(id, need)| {
            let id = JobId::new(id).map_err(|e| CiError::new(e.to_string()))?;
            Ok((
                id,
                Need {
                    result: need.result,
                    outputs: need.outputs,
                },
            ))
        })
        .collect()
}
