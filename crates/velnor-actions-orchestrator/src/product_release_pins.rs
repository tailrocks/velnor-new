//! Resolve the composed product-release workflow's platform and command pins.

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_mise::{
    MiseInstall, PinnedTool, PinnedToolExec, PrepareRustTarget, ToolCatalog,
};
use velnor_actions_workflow_renderer::ProductReleasePins;
use velnor_actions_workflow_renderer::toolchain_env::with_env_unset_argv;

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup_for_release_target;
use crate::utf8::strings_of;

/// Build every release command through the Mise adapter's typed requests.
pub(crate) fn resolve(config: &VelnorConfig) -> Result<ProductReleasePins, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    Ok(ProductReleasePins {
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
        install_qualify_tools_argv: qualify_install_argv(&catalog)?,
        install_runner_build_tools_argv: install_argv(
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            &catalog,
        )?,
        install_gh_argv: install_argv(&[PinnedTool::Gh], &catalog)?,
        build_argv: mbx_build_argv(&[], &catalog)?,
        intel_build_argv: mbx_build_argv(
            &["--target", ReleaseTarget::MacosX86_64.triple()],
            &catalog,
        )?,
        install_intel_target_argv: target_argv(
            ReleaseTarget::MacosArm64.triple(),
            ReleaseTarget::MacosX86_64.triple(),
            &catalog,
        )?,
        install_resource_probe_target_argv: resource_probe_target_argv(&catalog)?,
        runner_build_argv: runner_build_argv(&catalog)?,
        resource_probe_build_argv: resource_probe_build_argv(&catalog)?,
        actionlint_argv: exec_argv(
            &[PinnedTool::Actionlint, PinnedTool::Shellcheck],
            "actionlint",
            &["-color"],
            &catalog,
        )?,
        // Scrubbed like the validator vector: an empty-string GH_TOKEN
        // makes zizmor abort, while an absent one is clean offline.
        zizmor_argv: with_env_unset_argv(&exec_argv(
            &[PinnedTool::Zizmor],
            "zizmor",
            &[
                "--no-online-audits",
                "--config",
                ".zizmor.yml",
                ".github/workflows",
            ],
            &catalog,
        )?),
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog)?,
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        mr_boxington_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
    })
}

fn resource_probe_target_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    target_argv(
        ReleaseTarget::LinuxX86_64.triple(),
        "x86_64-unknown-linux-musl",
        catalog,
    )
}

fn resource_probe_build_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    mbx_locked_build_argv(
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-resource-probe",
            "--bin",
            "velnor-resource-probe",
            "--release",
            "--target",
            "x86_64-unknown-linux-musl",
        ],
        catalog,
    )
}

fn runner_build_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    mbx_locked_build_argv(
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--release",
            "--package",
            "velnor-runner-cli",
        ],
        catalog,
    )
}

fn install_argv(
    tools: &[PinnedTool],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let install = MiseInstall::new(tools.to_vec()).map_err(contract_error)?;
    strings_of(install.argv(catalog)).map_err(contract_error)
}

/// Exact `mise install` argv for candidate qualification tools.
fn qualify_install_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    install_argv(
        &[
            PinnedTool::Rust,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ],
        catalog,
    )
}

fn mbx_build_argv(extra: &[&str], catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec![
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
    ];
    args.extend(extra.iter().copied());
    mbx_locked_build_argv(&args, catalog)
}

fn mbx_locked_build_argv(
    args: &[&str],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        &[PinnedTool::Rust, PinnedTool::MrBoxington],
        "mbx",
        args,
        catalog,
    )
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

fn target_argv(
    host: &str,
    target: &str,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let request = PrepareRustTarget::new(host, target).map_err(contract_error)?;
    strings_of(request.argv(catalog)).map_err(contract_error)
}

fn contract_error(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}

#[cfg(test)]
#[path = "product_release_pins_mbx_tests.rs"]
mod mbx_tests;
