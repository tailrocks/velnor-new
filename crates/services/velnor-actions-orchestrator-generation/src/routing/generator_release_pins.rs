//! Resolve the generator-release workflow's platform and command pins.

use std::ffi::{OsStr, OsString};

use velnor_actions_contract_config::VelnorConfig;
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_mise::{
    MiseInstall, PinnedTool, PinnedToolExec, PrepareRustTarget, ToolCatalog,
};
use velnor_actions_workflow_generator::GeneratorReleasePins;

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::utf8::strings_of;
use velnor_actions_orchestrator_pins::pins::resolve_mise_setup_for_release_target;

/// Build every release command through the Mise adapter's typed requests.
pub(crate) fn resolve(config: &VelnorConfig) -> Result<GeneratorReleasePins, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    Ok(GeneratorReleasePins {
        linux_x86_64_setup: resolve_mise_setup_for_release_target(
            config,
            ReleaseTarget::LinuxX86_64,
        )?,
        macos_arm64_setup: resolve_mise_setup_for_release_target(
            config,
            ReleaseTarget::MacosArm64,
        )?,
        macos_x86_64_setup: resolve_mise_setup_for_release_target(
            config,
            ReleaseTarget::MacosX86_64,
        )?,
        install_gate_tools_argv: install_argv(
            &[
                PinnedTool::Gh,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ],
            &catalog,
        )?,
        install_build_tools_argv: install_argv(
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            &catalog,
        )?,
        install_gh_argv: install_argv(&[PinnedTool::Gh], &catalog)?,
        build_argv: exec_argv(
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            "mbx",
            &[
                "build",
                "--release",
                "--locked",
                "--package",
                "velnor-actions-cli",
                "--bin",
                "velnor-actions",
            ],
            &catalog,
        )?,
        macos_x86_64_cross_build_argv: exec_argv(
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            "mbx",
            &[
                "build",
                "--release",
                "--locked",
                "--package",
                "velnor-actions-cli",
                "--bin",
                "velnor-actions",
                "--target",
                ReleaseTarget::MacosX86_64.triple(),
            ],
            &catalog,
        )?,
        install_macos_x86_64_target_argv: install_macos_x86_64_target_argv(&catalog)?,
        actionlint_argv: exec_argv(
            &[PinnedTool::Actionlint, PinnedTool::Shellcheck],
            "actionlint",
            &["-color"],
            &catalog,
        )?,
        zizmor_argv: exec_argv(
            &[PinnedTool::Zizmor],
            "zizmor",
            &[
                "--no-online-audits",
                "--config",
                ".zizmor.yml",
                ".github/workflows",
            ],
            &catalog,
        )?,
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog)?,
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        mr_boxington_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
    })
}

fn install_macos_x86_64_target_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let request = PrepareRustTarget::new(
        ReleaseTarget::MacosArm64.triple(),
        ReleaseTarget::MacosX86_64.triple(),
    )
    .map_err(contract_error)?;
    strings_of(request.argv(catalog)).map_err(contract_error)
}

fn install_argv(
    tools: &[PinnedTool],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let install = MiseInstall::new(tools.to_vec()).map_err(contract_error)?;
    strings_of(install.argv(catalog)).map_err(contract_error)
}

fn exec_argv(
    tools: &[PinnedTool],
    program: &str,
    args: &[&str],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        tools.to_vec(),
        OsStr::new(program),
        args.iter().map(OsString::from).collect(),
    )
    .map_err(contract_error)?;
    strings_of(exec.argv(catalog)).map_err(contract_error)
}

fn contract_error(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}
