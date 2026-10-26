// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::ReleaseManifest;

use crate::OrchestratorError;

pub fn verify_local_generator_release_manifest(
    manifest_path: &Path,
    expected_source_commit: &str,
    linux_x64_binary: &Path,
    macos_arm64_binary: &Path,
) -> Result<(), OrchestratorError> {
    velnor_actions_workflow_renderer::release_helpers::verify_local_generator_release_manifest(
        manifest_path,
        expected_source_commit,
        linux_x64_binary,
        macos_arm64_binary,
    )
    .map_err(map_release_helper_error)
}

pub fn verify_binary_matches_consumer_manifest(
    manifest: &ReleaseManifest,
    exe_sha256: &str,
) -> Result<(), OrchestratorError> {
    velnor_actions_workflow_renderer::release_helpers::verify_binary_matches_consumer_manifest(
        manifest, exe_sha256,
    )
    .map_err(map_release_helper_error)
}

pub(crate) fn map_release_helper_error(
    error: velnor_actions_workflow_renderer::release_helpers::ReleaseHelperError,
) -> OrchestratorError {
    use velnor_actions_workflow_renderer::release_helpers::ReleaseHelperError;

    match error {
        ReleaseHelperError::Contract(error) => OrchestratorError::from(error),
        ReleaseHelperError::Problem(problem) => OrchestratorError::Contract { problem },
        ReleaseHelperError::Io { path, problem } => OrchestratorError::io(path, problem),
        ReleaseHelperError::UnsafePath { path, reason } => {
            OrchestratorError::UnsafePath { path, reason }
        }
        ReleaseHelperError::Render(error) => OrchestratorError::from(error),
    }
}
