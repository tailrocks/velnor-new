//! Private source-bound generator manifest entrypoints.

use std::env;
use std::process::ExitCode;

use velnor_actions_orchestrator::generator_release_manifest::{
    GENERATOR_RELEASE_MANIFEST_ASSEMBLE_OP, GENERATOR_RELEASE_MANIFEST_VERIFY_OP,
    GENERATOR_RELEASE_SIDECAR_WRITE_OP, assemble_generator_release_manifest,
    verify_generator_release_manifest, write_generator_release_sidecar,
};

/// Run a recognized manifest operation; unknown operations fall through to Clap.
pub(crate) fn try_internal() -> Option<ExitCode> {
    let operation = env::var("VELNOR_INTERNAL_OP").ok()?;
    let result = match operation.as_str() {
        GENERATOR_RELEASE_MANIFEST_ASSEMBLE_OP => assemble_generator_release_manifest(),
        GENERATOR_RELEASE_MANIFEST_VERIFY_OP => verify_generator_release_manifest(),
        GENERATOR_RELEASE_SIDECAR_WRITE_OP => write_generator_release_sidecar(),
        _ => return None,
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => crate::dispatch::fail_internal(&error.to_string()),
    })
}
