//! Job builders for the generic Rust binary-release workflow.

use super::yaml_steps::{
    checkout_step, download_step, job_base, mise_setup_step, run_step, run_step_with, upload_step,
};
use super::{BINARY_RELEASE_LINUX_RUNNER, RenderedCommands, RustBinaryReleaseRequest, scripts};
use crate::yaml::Yaml;

pub(super) fn verify_job(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
) -> Yaml {
    let script = scripts::verify_source(&request.config, binary, commands);
    let mut fields = job_base(
        "Select trusted release source",
        BINARY_RELEASE_LINUX_RUNNER,
        15,
        &[("contents", "read")],
        "github.event_name == 'schedule'",
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "should_release".to_owned(),
                Yaml::str("${{ steps.verify.outputs.should_release }}"),
            ),
            (
                "source_sha".to_owned(),
                Yaml::str("${{ steps.verify.outputs.source_sha }}"),
            ),
            (
                "default_sha".to_owned(),
                Yaml::str("${{ steps.verify.outputs.default_sha }}"),
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
            run_step("Install pinned GitHub CLI", &commands.install_gh),
            run_step_with(
                "Select verified package tag",
                "verify",
                &script,
                &[
                    ("GH_TOKEN", "${{ github.token }}"),
                    ("RUSTUP_TOOLCHAIN", &commands.rust_version),
                ],
            ),
        ]),
    ));
    Yaml::Map(fields)
}

pub(super) fn build_job(
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
    let mut fields = job_base(
        name,
        runner,
        60,
        &[("contents", "read")],
        "needs.verify-source.outputs.should_release == 'true'",
    );
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
            (
                "DEFAULT_SHA".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.default_sha }}"),
            ),
            (
                "RELEASE_TAG".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.tag }}"),
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
            run_step_with(
                "Verify selected source checkout",
                "source",
                &scripts::verify_build_source(&request.config, binary, commands),
                &[
                    ("SOURCE_SHA", "${{ env.SOURCE_SHA }}"),
                    ("RELEASE_VERSION", "${{ env.RELEASE_VERSION }}"),
                    ("RELEASE_TAG", "${{ env.RELEASE_TAG }}"),
                ],
            ),
            run_step_with("Build and verify binary", "build", &script, &build_env),
            upload_step(artifact, &path),
        ]),
    ));
    Yaml::Map(fields)
}

pub(super) fn publish_job(
    request: &RustBinaryReleaseRequest,
    commands: &RenderedCommands,
    binary: &str,
) -> Yaml {
    let prepare_script = scripts::prepare_assets(&request.config.package, binary);
    let publish_script = scripts::publish(&request.config.package, binary, &commands.gh_prefix);
    let mut fields = job_base(
        "Publish GitHub Release",
        BINARY_RELEASE_LINUX_RUNNER,
        30,
        &[("actions", "read"), ("contents", "write")],
        "needs.verify-source.outputs.should_release == 'true'",
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
                "DEFAULT_SHA".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.default_sha }}"),
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
