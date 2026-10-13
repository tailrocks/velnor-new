//! Emit the consumer's generated scheduled binary-release workflow.

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ReleaseTarget, WorkflowPolicy};
use velnor_actions_mise::{MiseInstall, PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::rust_binary_release::{
    BINARY_RELEASE_WORKFLOW_PATH, RustBinaryReleaseCommands, RustBinaryReleaseRequest,
    render_rust_binary_release_workflow,
};

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup_for_release_target;
use crate::prepare::GenerationPreparation;
use crate::utf8::strings_of;
use crate::workflow::CHECKOUT_USES;

/// Render the binary-release workflow when explicitly enabled.
///
/// # Errors
///
/// Returns a config error outside `consumer-v1`, a contract error for
/// invalid pinned commands, or a renderer error for invalid workflow data.
pub(crate) fn binary_release_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|stack| &stack.binary_release)
        .filter(|config| config.enabled)
    else {
        return Ok(Vec::new());
    };
    if prep.config.workflow.policy != WorkflowPolicy::ConsumerV1 {
        return Err(OrchestratorError::config(
            ".velnor/config.toml",
            "stacks.rust.binary_release.enabled",
            "binary_release_requires_consumer_policy",
        ));
    }

    let catalog = ToolCatalog::pinned();
    let commands = resolve_commands(config, &catalog)?;
    let request = RustBinaryReleaseRequest {
        config: config.clone(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        linux_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            ReleaseTarget::LinuxX86_64,
        )?,
        macos_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            ReleaseTarget::MacosArm64,
        )?,
        commands,
    };
    Ok(vec![RenderedFile {
        path: BINARY_RELEASE_WORKFLOW_PATH.to_owned(),
        bytes: render_rust_binary_release_workflow(&request)?,
    }])
}

fn resolve_commands(
    config: &velnor_actions_contract::RustBinaryReleaseConfig,
    catalog: &ToolCatalog,
) -> Result<RustBinaryReleaseCommands, OrchestratorError> {
    let install_rust = MiseInstall::new(vec![PinnedTool::Rust])
        .map_err(contract_error)?
        .argv(catalog);
    let install_gh = MiseInstall::new(vec![PinnedTool::Gh])
        .map_err(contract_error)?
        .argv(catalog);
    let metadata = cargo_argv(
        catalog,
        &[
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--manifest-path",
            &config.manifest_path,
        ],
    )?;
    let rustc_version = exec_argv(catalog, &[PinnedTool::Rust], "rustc", &["-vV"])?;
    let build = |target: &str| {
        let argv = cargo_argv(
            catalog,
            &[
                "build",
                "--locked",
                "--release",
                "--message-format=json",
                "--manifest-path",
                &config.manifest_path,
                "--package",
                &config.package,
                "--bin",
                config.binary_name(),
                "--target",
                target,
            ],
        )?;
        strings_of(argv).map_err(contract_error)
    };
    Ok(RustBinaryReleaseCommands {
        install_rust: strings_of(install_rust).map_err(contract_error)?,
        metadata: strings_of(metadata).map_err(contract_error)?,
        rustc_version,
        build_linux_x86_64: build(ReleaseTarget::LinuxX86_64.triple())?,
        build_macos_arm64: build(ReleaseTarget::MacosArm64.triple())?,
        install_gh: strings_of(install_gh).map_err(contract_error)?,
        gh_prefix: exec_argv(catalog, &[PinnedTool::Gh], "gh", &[])?,
        gh_version: catalog.version(PinnedTool::Gh).to_owned(),
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
    })
}

fn cargo_argv(catalog: &ToolCatalog, args: &[&str]) -> Result<Vec<OsString>, OrchestratorError> {
    exec_argv_os(catalog, &[PinnedTool::Rust], "cargo", args)
}

fn exec_argv(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, OrchestratorError> {
    strings_of(exec_argv_os(catalog, tools, program, args)?).map_err(contract_error)
}

fn exec_argv_os(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    program: &str,
    args: &[&str],
) -> Result<Vec<OsString>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        tools.to_vec(),
        OsStr::new(program),
        args.iter().map(OsString::from).collect(),
    )
    .map_err(contract_error)?;
    Ok(exec.argv(catalog))
}

fn contract_error(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}
