use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_mise::{
    MiseInstall, PinnedTool, PinnedToolExec, PrepareRustTarget, ToolCatalog,
};
use velnor_actions_workflow_renderer::toolchain_env::with_credential_unset_script;
use velnor_actions_workflow_renderer::{MiseSetup, ProductReleasePins};

use crate::OrchestratorError;
use crate::mbx_preflight::steps_for_catalog;
use crate::pins::resolve_mise_setup_for_release_target;
use crate::utf8::strings_of;
use velnor_actions_workflow_renderer::steps::MbxTransport;

fn resolve_setups(
    config: &VelnorConfig,
) -> Result<(MiseSetup, MiseSetup, MiseSetup), OrchestratorError> {
    Ok((
        resolve_mise_setup_for_release_target(config, ReleaseTarget::LinuxX86_64)?,
        resolve_mise_setup_for_release_target(config, ReleaseTarget::MacosArm64)?,
        resolve_mise_setup_for_release_target(config, ReleaseTarget::MacosX86_64)?,
    ))
}

pub(crate) fn resolve(
    config: &VelnorConfig,
    catalog: &ToolCatalog,
) -> Result<ProductReleasePins, OrchestratorError> {
    let catalog = catalog.clone();
    let (linux_x86_64_setup, macos_arm64_setup, macos_x86_64_setup) = resolve_setups(config)?;
    Ok(ProductReleasePins {
        linux_x86_64_setup,
        macos_arm64_setup,
        macos_x86_64_setup,
        install_gate_tools_argv: install_argv(
            &[
                PinnedTool::Gh,
                PinnedTool::Jactionlint,
                PinnedTool::Shellcheck,
            ],
            &catalog,
        )?,
        install_build_tools_argv: install_argv(
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            &catalog,
        )?,
        install_qualify_tools_argv: qualify_install_argv(&catalog)?,
        qualify_mbx_action_steps: {
            let steps = steps_for_catalog(&catalog, MbxTransport::InstallOnly, None)?;
            <[_; 3]>::try_from(steps).map_err(|_| OrchestratorError::Contract {
                problem: "release_mbx_steps_not_install_only".to_owned(),
            })?
        },
        install_runner_build_tools_argv: install_argv(&[PinnedTool::Rust], &catalog)?,
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
        runner_build_argv: mbx_locked_build_argv(
            &[
                "build",
                "--locked",
                "--manifest-path",
                "crates/velnor-runner/Cargo.toml",
                "--release",
                "--package",
                "velnor-runner-cli",
                "--bin",
                "velnor-host",
            ],
            &catalog,
        )?,
        resource_probe_build_argv: resource_probe_build_argv(&catalog)?,
        install_resource_probe_release_tools_argv: install_argv(
            &[
                PinnedTool::Rust,
                PinnedTool::MrBoxington,
                PinnedTool::Nextest,
            ],
            &catalog,
        )?,
        resource_probe_release_build_argv: resource_probe_release_build_argv(&catalog)?,
        resource_probe_release_test_argv: resource_probe_release_test_argv(&catalog)?,
        lint_argv: scrubbed_inline(crate::workflow_jobs::workflow_lint_argv(&catalog)?),
        verify_lint_argv: crate::workflow::jactionlint_verify_argv(&catalog),
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog)?,
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        verify_rust_argv: crate::workflow::verify_rust_argv(&catalog),
        mr_boxington_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
        gh_version: catalog.version(PinnedTool::Gh).to_owned(),
    })
}

pub(crate) fn homebrew_adoption_pins() -> Result<ProductReleasePins, OrchestratorError> {
    let config =
        crate::config::load_config_from_text(include_str!("../../../.velnor/config.toml"))?;
    resolve(&config, &ToolCatalog::pinned())
}

fn scrubbed_inline(mut argv: Vec<String>) -> Vec<String> {
    if let Some(script) = argv.get_mut(2) {
        *script = with_credential_unset_script(script);
    }
    argv
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

fn install_argv(
    tools: &[PinnedTool],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let install = MiseInstall::new(tools.to_vec()).map_err(contract_error)?;
    strings_of(install.argv(catalog)).map_err(contract_error)
}

fn qualify_install_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    install_argv(
        &[
            PinnedTool::Rust,
            PinnedTool::Jactionlint,
            PinnedTool::Shellcheck,
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

fn resource_probe_release_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    mbx_locked_build_argv(
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-runner-host",
            "--bin",
            "velnor-resource-probe-release",
            "--target",
            "x86_64-unknown-linux-gnu",
        ],
        catalog,
    )
}

fn resource_probe_release_test_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        &[
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Nextest,
        ],
        "mbx",
        &[
            "nextest",
            "run",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-runner-host",
            "--test",
            "resource_probe_release_public",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--no-tests",
            "fail",
        ],
        catalog,
    )
}
