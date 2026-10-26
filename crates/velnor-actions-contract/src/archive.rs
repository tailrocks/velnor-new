// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

use crate::cachekey::validate_semantic_text;
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveInputs {
    pub source_digest: String,

    pub package: String,

    pub target: String,

    pub features: Vec<String>,

    pub profile: String,

    pub toolchain_id: String,

    pub runtime: String,

    pub test_runner: String,

    pub format: String,

    pub platform_id: String,

    pub config_digest: String,
}

pub fn archive_id(inputs: &ArchiveInputs) -> Result<String, ContractError> {
    validate_digest(&inputs.source_digest)?;
    validate_digest(&inputs.toolchain_id)?;
    validate_digest(&inputs.platform_id)?;
    validate_digest(&inputs.config_digest)?;
    for (field, value) in [
        ("package", inputs.package.as_str()),
        ("target", inputs.target.as_str()),
        ("profile", inputs.profile.as_str()),
        ("runtime", inputs.runtime.as_str()),
        ("test_runner", inputs.test_runner.as_str()),
        ("format", inputs.format.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    let mut features = inputs.features.clone();
    features.sort();
    if features != inputs.features {
        return Err(ContractError::identity(
            "archive.features",
            "must_be_sorted",
        ));
    }
    for feature in &inputs.features {
        validate_semantic_text("archive.feature", feature)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}
