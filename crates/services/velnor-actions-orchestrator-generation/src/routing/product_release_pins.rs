//! Resolve the typed command inputs for the composed product-release route.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_generator::{GeneratorReleasePins, ProductReleasePins};
use velnor_actions_workflow_steps::toolchain_env::with_env_unset_argv;

use super::generator_release_pins::{exec_argv, install_argv};
use velnor_actions_orchestrator_core::OrchestratorError;

/// Extend the existing typed generator pins with the coordinator's extra commands.
pub(crate) fn resolve(
    generator: &GeneratorReleasePins,
) -> Result<ProductReleasePins, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    Ok(ProductReleasePins {
        linux_x86_64_setup: generator.linux_x86_64_setup.clone(),
        macos_arm64_setup: generator.macos_arm64_setup.clone(),
        macos_x86_64_setup: generator.macos_x86_64_setup.clone(),
        install_gate_tools_argv: generator.install_gate_tools_argv.clone(),
        install_build_tools_argv: generator.install_build_tools_argv.clone(),
        install_qualify_tools_argv: install_argv(
            &[
                PinnedTool::Rust,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ],
            &catalog,
        )?,
        install_runner_build_tools_argv: install_argv(&[PinnedTool::Rust], &catalog)?,
        install_gh_argv: generator.install_gh_argv.clone(),
        build_argv: generator.build_argv.clone(),
        intel_build_argv: generator.macos_x86_64_cross_build_argv.clone(),
        install_intel_target_argv: generator.install_macos_x86_64_target_argv.clone(),
        runner_build_argv: exec_argv(
            &[PinnedTool::Rust],
            "cargo",
            &[
                "build",
                "--locked",
                "--manifest-path",
                "crates/tools/velnor-runner-cli/Cargo.toml",
                "--release",
                "-p",
                "velnor-runner-cli",
            ],
            &catalog,
        )?,
        actionlint_argv: generator.actionlint_argv.clone(),
        zizmor_argv: with_env_unset_argv(&generator.zizmor_argv),
        gh_argv: generator.gh_argv.clone(),
        rust_version: generator.rust_version.clone(),
        mr_boxington_version: generator.mr_boxington_version.clone(),
    })
}

#[cfg(test)]
#[path = "product_release_pins/tests.rs"]
mod tests;
