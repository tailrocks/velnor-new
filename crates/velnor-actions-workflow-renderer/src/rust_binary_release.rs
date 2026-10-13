//! Tag-triggered consumer Cargo binary releases.
//!
//! Builds run on native pinned-toolchain hosts. Only the final job has
//! repository write permission, and it never executes downloaded bytes.

mod commands;
mod scripts;
#[cfg(test)]
#[path = "rust_binary_release_test.rs"]
mod tests;
mod yaml_steps;

use velnor_actions_contract::RustBinaryReleaseConfig;

use crate::setup::MiseSetup;
use crate::yaml::{Yaml, render_yaml};
use crate::{RenderError, join_argv_for_run, marker, steps};
use yaml_steps::{
    checkout_step, download_step, job_base, mise_setup_step, permission_map, run_step,
    run_step_with, upload_step,
};

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
    /// Verified Mise setup for Linux `x86_64`.
    pub linux_setup: MiseSetup,
    /// Verified Mise setup for macOS ARM64.
    pub macos_setup: MiseSetup,
    /// Exact tool commands selected by the Mise adapter.
    pub commands: RustBinaryReleaseCommands,
}

/// Render a marked generic Cargo binary-release workflow.
///
/// The trigger only accepts tags beginning with `<package>-v`; the
/// verification job then requires the complete tag to equal the selected
/// Cargo package version. All build jobs use the verified commit SHA.
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
    let tag_pattern = format!("{}-v*", request.config.package);
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Rust binary release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "push".to_owned(),
                Yaml::Map(vec![(
                    "tags".to_owned(),
                    Yaml::Seq(vec![Yaml::str(tag_pattern)]),
                )]),
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
                    verify_job(request, commands, binary),
                ),
                (
                    "build-linux".to_owned(),
                    build_job(
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
                    build_job(
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
                    publish_job(request, commands, binary),
                ),
            ]),
        ),
    ])
}

fn verify_job(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
) -> Yaml {
    let script = scripts::verify_source(&request.config, binary, commands);
    let mut fields = job_base(
        "Verify tag and Cargo version",
        BINARY_RELEASE_LINUX_RUNNER,
        15,
        &[("contents", "read")],
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "source_sha".to_owned(),
                Yaml::str("${{ steps.verify.outputs.source_sha }}"),
            ),
            (
                "version".to_owned(),
                Yaml::str("${{ steps.verify.outputs.version }}"),
            ),
            (
                "tag".to_owned(),
                Yaml::str("${{ steps.verify.outputs.tag }}"),
            ),
        ]),
    ));
    fields.push((
        "steps".to_owned(),
        Yaml::Seq(vec![
            checkout_step(&request.checkout_uses, "${{ github.sha }}"),
            mise_setup_step(&request.linux_setup),
            run_step("Install pinned Rust", &commands.install_rust),
            run_step_with(
                "Verify source, tag, package, and binary",
                "verify",
                &script,
                &[("RUSTUP_TOOLCHAIN", &commands.rust_version)],
            ),
        ]),
    ));
    Yaml::Map(fields)
}

fn build_job(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
    name: &str,
    runner: &str,
    target: &str,
    artifact: &str,
) -> Yaml {
    let (build, setup) = if target == "x86_64-unknown-linux-gnu" {
        (&commands.build_linux, &request.linux_setup)
    } else {
        (&commands.build_macos, &request.macos_setup)
    };
    let script = scripts::build(binary, target, build, &commands.rustc_version);
    let mut fields = job_base(name, runner, 60, &[("contents", "read")]);
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(vec![Yaml::str("verify-source")]),
    ));
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        )]),
    ));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![
            (
                "RELEASE_VERSION".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.version }}"),
            ),
            (
                "SOURCE_SHA".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.source_sha }}"),
            ),
        ]),
    ));
    let path =
        format!("dist/{binary}-${{{{ needs.verify-source.outputs.version }}}}-{target}.tar.gz");
    let mut build_env = vec![("RUSTUP_TOOLCHAIN", commands.rust_version.as_str())];
    if let Some(variable) = request.config.source_commit_env.as_deref() {
        build_env.push((variable, "${{ env.SOURCE_SHA }}"));
    }
    fields.push((
        "steps".to_owned(),
        Yaml::Seq(vec![
            checkout_step(
                &request.checkout_uses,
                "${{ needs.verify-source.outputs.source_sha }}",
            ),
            mise_setup_step(setup),
            run_step("Install pinned Rust", &commands.install_rust),
            run_step_with("Build and verify binary", "build", &script, &build_env),
            upload_step(artifact, &path),
        ]),
    ));
    Yaml::Map(fields)
}

fn publish_job(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
) -> Yaml {
    let prepare_script = scripts::prepare_assets(&request.config.package, binary);
    let publish_script = scripts::publish(binary, &commands.gh_prefix);
    let mut fields = job_base(
        "Publish GitHub Release",
        BINARY_RELEASE_LINUX_RUNNER,
        30,
        &[("actions", "read"), ("contents", "write")],
    );
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(vec![
            Yaml::str("verify-source"),
            Yaml::str("build-linux"),
            Yaml::str("build-macos"),
        ]),
    ));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![
            (
                "RELEASE_VERSION".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.version }}"),
            ),
            (
                "SOURCE_SHA".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.source_sha }}"),
            ),
            (
                "RELEASE_TAG".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.tag }}"),
            ),
        ]),
    ));
    fields.push((
        "steps".to_owned(),
        Yaml::Seq(vec![
            mise_setup_step(&request.linux_setup),
            run_step("Install pinned GitHub CLI", &commands.install_gh),
            download_step(
                "Download Linux x86_64 asset",
                "${{ needs.build-linux.outputs.artifact_id }}",
                "assets/incoming-linux",
            ),
            download_step(
                "Download macOS ARM64 asset",
                "${{ needs.build-macos.outputs.artifact_id }}",
                "assets/incoming-macos",
            ),
            run_step("Validate archives and prepare checksums", &prepare_script),
            run_step_with(
                "Verify checksums, recheck tag, and publish",
                "publish",
                &publish_script,
                &[("GH_TOKEN", "${{ github.token }}")],
            ),
        ]),
    ));
    Yaml::Map(fields)
}
