// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::verifier_builders;

use crate::OrchestratorError;

pub(crate) use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

pub(crate) const NOT_CATALOGUED_CODE: &str = verifier_builders::NOT_CATALOGUED_CODE;
pub(crate) const VERIFY_RUST_MANIFEST_STEP: &str =
    velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP;
pub(crate) const VERIFY_RESTORED_RUST_STEP: &str = verifier_builders::VERIFY_RESTORED_RUST_STEP;

pub(crate) fn toolchain_script(
    version: &str,
    phase: ToolchainPhase,
    manifests: &[(&str, &str, &str)],
) -> String {
    verifier_builders::toolchain_script(version, phase, manifests)
}

pub(crate) fn verifier_shell_step(
    catalog: &ToolCatalog,
    phase: ToolchainPhase,
) -> Result<Step, OrchestratorError> {
    verifier_builders::rust_verifier_shell_step(catalog, phase).map_err(OrchestratorError::from)
}

pub(crate) fn verify_rust_owned_argv(catalog: &ToolCatalog, phase: ToolchainPhase) -> Vec<String> {
    verifier_builders::verify_rust_owned_argv(catalog, phase)
}

pub(crate) fn verify_rust_argv(catalog: &ToolCatalog) -> Vec<String> {
    verifier_builders::verify_rust_argv(catalog)
}

pub(crate) fn verify_rust_manifest_step(_catalog: &ToolCatalog) -> Step {
    verifier_builders::verify_rust_manifest_step()
}

pub(crate) fn verify_restored_rust_step(_catalog: &ToolCatalog) -> Step {
    verifier_builders::verify_restored_rust_step()
}
