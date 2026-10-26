// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::cachekey::validate_semantic_text;
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformInputs {
    pub os: String,

    pub arch: String,

    pub runs_on: String,

    pub image_os: String,

    pub target: String,
}

pub fn platform_id(inputs: &PlatformInputs) -> Result<String, ContractError> {
    for (field, value) in [
        ("os", inputs.os.as_str()),
        ("arch", inputs.arch.as_str()),
        ("runs_on", inputs.runs_on.as_str()),
        ("image_os", inputs.image_os.as_str()),
        ("target", inputs.target.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

#[path = "cachekey_observed.rs"]
mod observed;
pub use observed::{MAX_OBSERVED_PLATFORM_FACT_BYTES, observed_platform_id};
