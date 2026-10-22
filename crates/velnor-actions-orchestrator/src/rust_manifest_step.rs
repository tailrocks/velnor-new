//! `Verify Rust channel manifest`: every `core:rust` toolchain is checked
//! file by file before anything compiles with it.
//!
//! Mise's `core:rust` backend installs through rustup and writes no checksum
//! into `mise.lock`, and the tools cache restores toolchains that any
//! default-branch job running code could have written. Checking rustup's own
//! metadata proves nothing about the bytes that will run, so the step
//! ([`rust_toolchain_script`]) fetches `channel-rust-<version>.toml`, checks
//! it against the catalog digest
//! ([`velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS`]), downloads the
//! component tarballs the installed toolchain lists, checks each against the
//! SHA-256 that verified manifest pins, and requires the toolchain directory
//! to hold exactly those files with exactly those bytes. A version the
//! catalog does not know fails the job.
//!
//! Owned-home jobs also run it right after the tools cache restore, before
//! Mise or rustup touches the toolchain: that run discards the cached rustup
//! and its proxies (Mise reinstalls them fresh) and verifies a restored
//! toolchain, so no restored Rust byte runs unchecked.
//!
//! The generated text stays version-free (hard decision 4): the step reads
//! the run-time `${VELNOR_TOOL_RUST}` and carries the whole digest table.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, ToolCatalog, ToolHomes};

use crate::OrchestratorError;

#[path = "rust_toolchain_script.rs"]
mod rust_toolchain_script;
pub(crate) use rust_toolchain_script::{NOT_CATALOGUED_CODE, ToolchainPhase, toolchain_script};

/// Contract-fixed display name of the installed-toolchain verification step.
pub(crate) const VERIFY_RUST_MANIFEST_STEP: &str =
    velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP;

/// Display name of the verification that runs right after the cache restore.
pub(crate) const VERIFY_RESTORED_RUST_STEP: &str = "Verify restored Rust toolchain";

fn owned_home_step(
    name: &str,
    catalog: &ToolCatalog,
    phase: ToolchainPhase,
) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::runner_temp();
    let env = BTreeMap::from([
        ("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned()),
        ("CARGO_HOME".to_owned(), homes.cargo_home().to_owned()),
    ]);
    let script = toolchain_script(
        catalog.version(PinnedTool::Rust),
        phase,
        RUST_CHANNEL_MANIFESTS,
    );
    let mut step = velnor_actions_workflow_renderer::steps::shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    // Both phases are the same cache-safe check (the dependency-seed job
    // admits only cache roles).
    step.role = Some(StepRole::VerifyRustManifest);
    Ok(step)
}

/// Canonical scrubbed Rust verifier body used to render one fixed composite.
pub(crate) fn verifier_shell_step(
    catalog: &ToolCatalog,
    phase: ToolchainPhase,
) -> Result<Step, OrchestratorError> {
    let name = match phase {
        ToolchainPhase::Restored => VERIFY_RESTORED_RUST_STEP,
        ToolchainPhase::Installed => VERIFY_RUST_MANIFEST_STEP,
    };
    owned_home_step(name, catalog, phase)
}

/// `sh -c` argv of one phase of the check for a renderer-built job on the
/// owned homes (the step sets `RUSTUP_HOME` and `CARGO_HOME`).
pub(crate) fn verify_rust_owned_argv(catalog: &ToolCatalog, phase: ToolchainPhase) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        toolchain_script(
            catalog.version(PinnedTool::Rust),
            phase,
            RUST_CHANNEL_MANIFESTS,
        ),
    ]
}

/// `sh -c` argv of the installed-toolchain check for jobs on the runner's
/// default rustup home (release, qualification and freshness jobs), which
/// install Rust without the owned homes or the tools cache.
pub(crate) fn verify_rust_argv(catalog: &ToolCatalog) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        toolchain_script(
            catalog.version(PinnedTool::Rust),
            ToolchainPhase::Installed,
            RUST_CHANNEL_MANIFESTS,
        ),
    ]
}

/// Typed, credential-scrubbed verification of the job's installed Rust.
///
/// # Errors
///
/// Returns a contract error when the renderer rejects the fixed argv.
pub(crate) fn verify_rust_manifest_step(_catalog: &ToolCatalog) -> Step {
    crate::verifier_actions::call_step(
        VERIFY_RUST_MANIFEST_STEP,
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
        Some(StepRole::VerifyRustManifest),
    )
}

/// Verification of whatever the tools cache restored, emitted right before
/// `Prepare pinned tools` (which runs after the restore).
///
/// # Errors
///
/// Returns a contract error when the renderer rejects the fixed argv.
pub(crate) fn verify_restored_rust_step(_catalog: &ToolCatalog) -> Step {
    crate::verifier_actions::call_step(
        VERIFY_RESTORED_RUST_STEP,
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
        Some(StepRole::VerifyRustManifest),
    )
}

#[cfg(test)]
#[path = "rust_manifest_step_tests.rs"]
mod tests;
