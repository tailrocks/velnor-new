// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json};
use velnor_actions_mise::CheckDeadline;

use super::{HomebrewSeedOutputBaseIdentity, PrivateVerifiedSeedOutputBase};
use crate::{OrchestratorError, internal::internal};

#[cfg(test)]
#[path = "homebrew_cutover_marker_tests.rs"]
mod tests;

pub(crate) const MARKER_PATH: &str = ".velnor/homebrew-cutover.json";
pub(crate) const MARKER_LIMIT: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CutoverMarker {
    pub(crate) schema: String,
    pub(crate) seed: HomebrewSeedOutputBaseIdentity,
    pub(crate) source_sha: String,
    pub(crate) locator_blob_sha: String,
    pub(crate) preview_receipt_digest: String,
    pub(crate) preview_formula_sha256: String,
}

pub(crate) fn marker_bytes(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    preview: &[u8],
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let (receipt, canonical) =
        crate::homebrew::runtime::render_current_preview_formula(authority, deadline)?;
    if canonical != preview {
        return Err(internal("homebrew_cutover_marker_preview_mismatch"));
    }
    let marker = CutoverMarker {
        schema: "velnor.homebrew-cutover-initialization.v1".to_owned(),
        seed: witness.identity().clone(),
        source_sha: authority.authority_sha().to_owned(),
        locator_blob_sha: authority.locator_blob_sha().to_owned(),
        preview_receipt_digest: receipt,
        preview_formula_sha256: sha256(preview),
    };
    canonical_json_bytes(&marker).map_err(|_| internal("homebrew_cutover_marker_encode"))
}

pub(crate) fn parse_marker(bytes: &[u8]) -> Result<CutoverMarker, OrchestratorError> {
    let invalid = || internal("homebrew_cutover_marker_invalid");
    if bytes.is_empty() || bytes.len() > MARKER_LIMIT {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let value = parse_strict_json(text).map_err(|_| invalid())?;
    let marker: CutoverMarker = serde_json::from_value(value).map_err(|_| invalid())?;
    if marker.schema != "velnor.homebrew-cutover-initialization.v1"
        || canonical_json_bytes(&marker).map_err(|_| invalid())? != bytes
    {
        return Err(invalid());
    }
    Ok(marker)
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
            out
        })
}
