use super::{OciCandidateError, Result};

pub(super) fn parse(bytes: &[u8]) -> Result<Value> {
    super::super::strict_json::parse(bytes).map_err(|error| {
        if error.starts_with("trailing JSON:") {
            OciCandidateError("oci_json_trailing")
        } else {
            OciCandidateError("oci_json_invalid")
        }
    })
}

use serde_json::Value;
