// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ContractError;

pub use velnor_runner_image_manifest::strict_json::{
    MAX_JSON_NESTING_DEPTH, MAX_UNTRUSTED_DOCUMENT_BYTES,
};

pub fn check_document_size(len: usize, limit: usize) -> Result<(), ContractError> {
    velnor_runner_image_manifest::strict_json::check_document_size(len, limit).map_err(Into::into)
}

pub fn parse_strict_json(text: &str) -> Result<serde_json::Value, ContractError> {
    velnor_runner_image_manifest::strict_json::parse_strict_json(text).map_err(Into::into)
}

pub fn parse_strict_json_with_limit(
    text: &str,
    limit: usize,
) -> Result<serde_json::Value, ContractError> {
    velnor_runner_image_manifest::strict_json::parse_strict_json_with_limit(text, limit)
        .map_err(Into::into)
}

pub fn parse_strict_json_bytes(
    bytes: &[u8],
    limit: usize,
) -> Result<serde_json::Value, ContractError> {
    velnor_runner_image_manifest::strict_json::parse_strict_json_bytes(bytes, limit)
        .map_err(Into::into)
}
