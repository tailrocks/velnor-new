//! Native helpers used by repository-policy operations.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
mod strict_json;
use strict_json::parse_strict_json_bytes;

const MAX_UNTRUSTED_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

const MAX_CARGO_MESSAGES_BYTES: u64 = 64 * 1024 * 1024;

/// Resolve a path supplied to the private repository-policy operation.
fn rooted_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

mod cargo_artifact;
#[cfg(test)]
use cargo_artifact::cargo_artifact_executable;
pub(super) use cargo_artifact::print_cargo_artifact_executable;

#[cfg(test)]
#[path = "release_support_tests.rs"]
mod tests;
