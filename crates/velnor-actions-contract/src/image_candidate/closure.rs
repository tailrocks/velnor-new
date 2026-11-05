// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::ContractError;

#[path = "closure_inventory.rs"]
mod inventory;

pub const IMAGE_NATIVE_CLOSURE_SCHEMA: &str = "velnor.job-image-native-source-closure.v1";

pub use inventory::native_image_candidate_inputs;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateSourceClosure {
    pub schema: String,

    pub source_commit: String,

    pub inputs: BTreeMap<String, String>,
}

impl ImageCandidateSourceClosure {
    pub fn new(
        source_commit: String,
        inputs: BTreeMap<String, String>,
    ) -> Result<Self, ContractError> {
        let closure = Self {
            schema: IMAGE_NATIVE_CLOSURE_SCHEMA.to_owned(),
            source_commit,
            inputs,
        };
        validate_image_candidate_source_closure(&closure)?;
        Ok(closure)
    }
}

pub fn parse_image_candidate_source_closure(
    bytes: &[u8],
) -> Result<ImageCandidateSourceClosure, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)?;
    let closure: ImageCandidateSourceClosure = serde_json::from_value(value).map_err(|error| {
        ContractError::CanonicalJson(format!("image_candidate_closure:{error}"))
    })?;
    validate_image_candidate_source_closure(&closure)?;
    Ok(closure)
}

pub fn validate_image_candidate_source_closure(
    closure: &ImageCandidateSourceClosure,
) -> Result<(), ContractError> {
    let expected = native_image_candidate_inputs()
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    let actual = closure.inputs.keys().cloned().collect::<BTreeSet<_>>();
    if closure.schema != IMAGE_NATIVE_CLOSURE_SCHEMA
        || !is_hex(&closure.source_commit, 40)
        || actual != expected
        || closure.inputs.values().any(|digest| !is_hex(digest, 64))
    {
        return Err(ContractError::CanonicalJson(
            "image_candidate_closure_identity".to_owned(),
        ));
    }
    Ok(())
}

pub fn serialize_image_candidate_source_closure(
    closure: &ImageCandidateSourceClosure,
) -> Result<Vec<u8>, ContractError> {
    validate_image_candidate_source_closure(closure)?;
    let mut bytes = crate::canonical_json_bytes(closure)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
