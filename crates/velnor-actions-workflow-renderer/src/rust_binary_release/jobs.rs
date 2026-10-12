//! Publish-job YAML for generic Rust binary releases.

use super::yaml_steps::{download_step, job_base, mise_setup_step, run_step, run_step_with};
use super::{BINARY_RELEASE_LINUX_RUNNER, RenderedCommands, RustBinaryReleaseRequest, scripts};
use crate::yaml::Yaml;

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
            (
                "RESUME_RELEASE_ID".to_owned(),
                Yaml::str("${{ needs.verify-source.outputs.resume_release_id }}"),
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
