//! Trusted scheduled consumer Cargo binary releases.
//!
//! Builds run on native pinned-toolchain hosts. Only the final job has
//! repository write permission, and it never checks out source or executes
//! downloaded bytes.

mod commands;
#[cfg(test)]
#[path = "rust_binary_release_deterministic_archive_test.rs"]
mod deterministic_archive_tests;
mod jobs;
#[cfg(test)]
#[path = "rust_binary_release_prepare_assets_test.rs"]
mod prepare_assets_tests;
#[cfg(test)]
#[path = "rust_binary_release_resume_test.rs"]
mod resume_tests;
mod scripts;
#[cfg(test)]
#[path = "rust_binary_release_scripts_test.rs"]
mod scripts_test;
#[cfg(test)]
#[path = "rust_binary_release_test.rs"]
mod tests;
mod yaml_steps;

use velnor_actions_contract::RustBinaryReleaseConfig;

use crate::setup::MiseSetup;
use crate::yaml::{Yaml, render_yaml};
use crate::{RenderError, join_argv_for_run, marker, steps};
use yaml_steps::permission_map;

/// Fixed GitHub-hosted Linux `x86_64` runner.
pub const BINARY_RELEASE_LINUX_RUNNER: &str = "ubuntu-24.04";
/// Fixed GitHub-hosted macOS ARM64 runner.
pub const BINARY_RELEASE_MACOS_RUNNER: &str = "macos-15";
/// Generated generic Rust binary-release workflow path.
pub const BINARY_RELEASE_WORKFLOW_PATH: &str = ".github/workflows/binary-release.yml";

/// Mise and Cargo command identities resolved by the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustBinaryReleaseCommands {
    /// Exact Mise installation command for Rust.
    pub install_rust: Vec<String>,
    /// Pinned Cargo metadata command with `--locked` and the selected manifest.
    pub metadata: Vec<String>,
    /// Pinned `rustc -vV` command used to verify native runner architecture.
    pub rustc_version: Vec<String>,
    /// Pinned Cargo release build for `x86_64-unknown-linux-gnu`.
    pub build_linux_x86_64: Vec<String>,
    /// Pinned Cargo release build for `aarch64-apple-darwin`.
    pub build_macos_arm64: Vec<String>,
    /// Exact Mise installation command for GitHub CLI.
    pub install_gh: Vec<String>,
    /// Pinned Mise command prefix ending in `gh` for shell API calls.
    pub gh_prefix: Vec<String>,
    /// Exact GitHub CLI version forced by Mise.
    pub gh_version: String,
    /// Exact Rust version forced through `RUSTUP_TOOLCHAIN`.
    pub rust_version: String,
}

/// Inputs for pure rendering of one generic Cargo binary-release workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustBinaryReleaseRequest {
    /// Validated release selection.
    pub config: RustBinaryReleaseConfig,
    /// Generator version for the marker.
    pub generator_version: String,
    /// Pinned checkout action selected by the orchestrator.
    pub checkout_uses: String,
    /// Pinned artifact download action selected by the orchestrator.
    pub download_artifact_uses: String,
    /// Pinned artifact upload action selected by the orchestrator.
    pub upload_artifact_uses: String,
    /// Verified Mise setup for Linux `x86_64`.
    pub linux_setup: MiseSetup,
    /// Verified Mise setup for macOS ARM64.
    pub macos_setup: MiseSetup,
    /// Exact tool commands selected by the Mise adapter.
    pub commands: RustBinaryReleaseCommands,
}

/// Render a marked generic Cargo binary-release workflow.
///
/// The trusted default-branch workflow polls release tags on a schedule. It
/// selects one unreleased package tag whose commit is reachable from the
/// captured default-branch SHA and whose manifest version matches the tag.
/// All build jobs use that exact commit SHA.
/// # Errors
pub fn render_rust_binary_release_workflow(
    request: &RustBinaryReleaseRequest,
) -> Result<String, RenderError> {
    request
        .config
        .validate("binary-release")
        .map_err(RenderError::Contract)?;
    if !request.config.enabled {
        return Err(RenderError::InvalidWorkflow(
            "binary_release_disabled".to_owned(),
        ));
    }
    marker::validate_version(&request.generator_version)?;
    steps::validate_uses(&request.checkout_uses)?;
    if !request.checkout_uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef("not_checkout".to_owned()));
    }
    steps::validate_uses(&request.download_artifact_uses)?;
    if !request
        .download_artifact_uses
        .starts_with("actions/download-artifact@")
    {
        return Err(RenderError::BadActionRef(
            "not_download_artifact".to_owned(),
        ));
    }
    steps::validate_uses(&request.upload_artifact_uses)?;
    if !request
        .upload_artifact_uses
        .starts_with("actions/upload-artifact@")
    {
        return Err(RenderError::BadActionRef("not_upload_artifact".to_owned()));
    }
    request.linux_setup.validate()?;
    request.macos_setup.validate()?;
    commands::validate_commands(&request.config, &request.commands)?;
    let commands = render_commands(&request.commands)?;
    let config = &request.config;
    let binary = config.binary_name();
    let document = workflow_document(request, &commands, binary);
    let rendered = marker::with_marker(&request.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(BINARY_RELEASE_WORKFLOW_PATH, &rendered)?;
    steps::scan_for_private_subcommands(&rendered)?;
    Ok(rendered)
}

struct RenderedCommands {
    install_rust: String,
    metadata: String,
    rustc_version: String,
    rust_version: String,
    build_linux: String,
    build_macos: String,
    install_gh: String,
    gh_prefix: String,
}

fn render_commands(commands: &RustBinaryReleaseCommands) -> Result<RenderedCommands, RenderError> {
    Ok(RenderedCommands {
        install_rust: join_argv_for_run(&commands.install_rust)?,
        metadata: join_argv_for_run(&commands.metadata)?,
        rustc_version: join_argv_for_run(&commands.rustc_version)?,
        rust_version: commands.rust_version.clone(),
        build_linux: join_argv_for_run(&commands.build_linux_x86_64)?,
        build_macos: join_argv_for_run(&commands.build_macos_arm64)?,
        install_gh: join_argv_for_run(&commands.install_gh)?,
        gh_prefix: join_argv_for_run(&commands.gh_prefix)?,
    })
}

fn workflow_document(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Rust binary release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "schedule".to_owned(),
                Yaml::Seq(vec![Yaml::Map(vec![(
                    "cron".to_owned(),
                    Yaml::str("17 * * * *"),
                )])]),
            )]),
        ),
        (
            "permissions".to_owned(),
            permission_map(&[("contents", "read")]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("binary-release-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![
                (
                    "verify-source".to_owned(),
                    jobs::verify_job(request, commands, binary),
                ),
                (
                    "build-linux".to_owned(),
                    jobs::build_job(
                        request,
                        commands,
                        binary,
                        "Build Linux x86_64 binary",
                        BINARY_RELEASE_LINUX_RUNNER,
                        "x86_64-unknown-linux-gnu",
                        "binary-release-linux-x86_64",
                    ),
                ),
                (
                    "build-macos".to_owned(),
                    jobs::build_job(
                        request,
                        commands,
                        binary,
                        "Build macOS ARM64 binary",
                        BINARY_RELEASE_MACOS_RUNNER,
                        "aarch64-apple-darwin",
                        "binary-release-macos-arm64",
                    ),
                ),
                (
                    "publish-release".to_owned(),
                    jobs::publish_job(request, commands, binary),
                ),
            ]),
        ),
    ])
}
