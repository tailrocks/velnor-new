//! Local release-manifest verification command.

use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator::local_release_manifest::verify_local_generator_release_manifest;

use crate::dispatch::fail_public;

/// Verify local binary bytes against a caller-provided release manifest.
pub(crate) fn run_verify_release_manifest(
    manifest: &Path,
    expected_source_commit: &str,
    linux_x64_binary: &Path,
    macos_arm64_binary: &Path,
) -> ExitCode {
    match verify_local_generator_release_manifest(
        manifest,
        expected_source_commit,
        linux_x64_binary,
        macos_arm64_binary,
    ) {
        Ok(()) => {
            println!("Local generator binaries match the release manifest.");
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
