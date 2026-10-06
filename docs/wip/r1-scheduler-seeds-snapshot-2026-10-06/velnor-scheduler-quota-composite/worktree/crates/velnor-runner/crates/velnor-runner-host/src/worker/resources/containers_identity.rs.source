//! Exact launch-role label checks for existing containers.

use std::collections::HashMap;

use crate::error::HostError;
use crate::worker::launch_identity_labels_match;

pub(super) fn same_launch(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> bool {
    launch_identity_labels_match(expected, actual)
}

pub(super) fn validate_existing_row(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> Result<&'static str, HostError> {
    if !same_launch(expected, actual) {
        return Err(HostError::Ownership);
    }
    match actual.get("velnor.role").map(String::as_str) {
        Some("dind") => Ok("dind"),
        Some("runner") => Ok("runner"),
        _ => Err(HostError::Ownership),
    }
}
