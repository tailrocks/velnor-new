//! Private source-bound generator manifest entrypoints.

use std::env;
use std::process::ExitCode;

use velnor_actions_orchestrator::generator_release_manifest::{
    GENERATOR_RELEASE_MANIFEST_ASSEMBLE_OP, GENERATOR_RELEASE_MANIFEST_VERIFY_OP,
    assemble_generator_release_manifest, verify_generator_release_manifest,
};

/// Run a recognized manifest operation; unknown operations fall through to Clap.
pub(crate) fn try_internal() -> Option<ExitCode> {
    let operation = env::var("VELNOR_INTERNAL_OP").ok()?;
    let result = match operation.as_str() {
        GENERATOR_RELEASE_MANIFEST_ASSEMBLE_OP => assemble_generator_release_manifest(),
        GENERATOR_RELEASE_MANIFEST_VERIFY_OP => verify_generator_release_manifest(),
        _ => return None,
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => crate::dispatch::fail_internal(&error.to_string()),
    })
}
