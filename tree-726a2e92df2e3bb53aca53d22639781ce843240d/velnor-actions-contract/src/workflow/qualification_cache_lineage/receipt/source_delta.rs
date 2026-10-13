//! Source-delta and bounded input parsing helpers.

use super::errors::invalid;
use super::types::QualificationSourceDelta;
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

pub(super) fn validate_source_delta(
    delta: &QualificationSourceDelta,
    expected_base: &str,
    expected_source: &str,
) -> Result<(), ContractError> {
    if !delta.base_is_ancestor
        || delta.base_source_sha != expected_base
        || delta.source_sha != expected_source
        || delta.base_source_sha == delta.source_sha
        || delta.changed_paths.is_empty()
        || delta.changed_paths.len() > 256
        || delta
            .changed_paths
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid("useful_source_delta_unbound_or_unordered"));
    }
    for path in &delta.changed_paths {
        if path.len() > 256 || crate::canonical::normalize_posix_path(path)? != *path {
            return Err(invalid("useful_source_delta_path_invalid"));
        }
    }
    validate_digest(&delta.diff_digest)?;
    if delta.diff_digest != digest_b3(&canonical_json_bytes(&delta.changed_paths)?) {
        return Err(invalid("useful_source_delta_digest_mismatch"));
    }
    Ok(())
}
