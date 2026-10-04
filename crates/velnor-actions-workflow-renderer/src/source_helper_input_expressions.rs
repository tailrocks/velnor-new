//! Exact operation-bound inputs for the sealed Prepared and Verify workers.
use crate::{RenderError, commands};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation};

pub(super) fn validate(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    validate_operation(record.invocation().descriptor().operation(), environment)
}

fn validate_operation(
    operation: SourceBoundOperation,
    environment: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if !matches!(
        operation,
        SourceBoundOperation::RustReleasePreparedPackage
            | SourceBoundOperation::RustReleasePackageVerify
    ) {
        return commands::validate_env(environment);
    }
    let mut structural = environment.clone();
    for (key, value) in environment {
        if let Some(expected) = source_binding(key) {
            admit(&mut structural, key, value, &expected)?;
        } else if let Some(expected) = prepared_binding(key) {
            if operation != SourceBoundOperation::RustReleasePackageVerify {
                return Err(invalid(key));
            }
            admit(&mut structural, key, value, &expected)?;
        }
    }
    // Fixed admitted values contain no private commands or control characters.
    // Keep their keys for every reserved-key and structural check; ordinary
    // values still pass through the unchanged expression and command policy.
    commands::validate_env(&structural)
}

fn admit(
    structural: &mut BTreeMap<String, String>,
    key: &str,
    value: &str,
    expected: &str,
) -> Result<(), RenderError> {
    if value != format!("${{{{ {expected} }}}}") {
        return Err(invalid(key));
    }
    structural.insert(key.to_owned(), String::new());
    Ok(())
}

fn source_binding(key: &str) -> Option<String> {
    let output = match key {
        "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID" => "source-snapshot-artifact-id",
        "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST" => "source-snapshot-artifact-digest",
        "RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256" => "source-snapshot-blob-sha256",
        "RELEASE_SOURCE_COMMIT_SHA" => "source-commit-sha",
        "RELEASE_SOURCE_TREE_SHA" => "source-tree-sha",
        "GITHUB_REF" => return Some("github.ref".to_owned()),
        "GITHUB_WORKFLOW_REF" => return Some("github.workflow_ref".to_owned()),
        "GITHUB_WORKFLOW_SHA" => return Some("github.workflow_sha".to_owned()),
        "RUNNER_TEMP" => return Some("runner.temp".to_owned()),
        _ => return None,
    };
    Some(format!("needs.release-source-snapshot.outputs.{output}"))
}

fn prepared_binding(key: &str) -> Option<String> {
    let output = match key {
        "RELEASE_PACKAGE_ARTIFACT_ID" => "package-artifact-id",
        "RELEASE_PACKAGE_ARTIFACT_DIGEST" => "package-artifact-digest",
        "RELEASE_PACKAGE_BLOB_SHA256" => "package-blob-sha256",
        _ => return None,
    };
    Some(format!("needs.release-package.outputs.{output}"))
}

fn invalid(key: &str) -> RenderError {
    RenderError::BadCommand(format!("source_helper_input_expression:{key}"))
}

#[cfg(test)]
#[path = "source_helper_input_expression_tests.rs"]
mod tests;
