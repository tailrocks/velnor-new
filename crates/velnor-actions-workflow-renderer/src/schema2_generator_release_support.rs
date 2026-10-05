//! Release-job construction shared by the schema-2 generator workflow.

use crate::yaml::Yaml;

use super::assets::ReleaseAssets;

#[path = "schema2_generator_release_support_steps.rs"]
mod steps;

pub(super) fn eligibility_job(runs_on: Yaml) -> (String, Yaml) {
    steps::eligibility_job(runs_on)
}

pub(super) fn linux_build_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    build_job("linux", "Build Linux generator", runs_on, assets)
}

pub(super) fn macos_build_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    build_job("macos", "Build macOS ARM64 generator", runs_on, assets)
}

fn build_job(
    target: &str,
    name: &str,
    runs_on: Yaml,
    assets: &ReleaseAssets,
) -> (String, Yaml) {
    let (binary, sidecar, platform, verify) = match target {
        "linux" => (
            &assets.linux_binary,
            &assets.linux_sidecar,
            "linux",
            "ELF*x86-64",
        ),
        _ => (
            &assets.macos_binary,
            &assets.macos_sidecar,
            "macos",
            "Mach-O*arm64",
        ),
    };
    let upload_name = format!("generator-{platform}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}");
    let job_steps = vec![
        steps::checkout_step(),
        steps::mise_step(),
        steps::run_step("Install pinned Rust", "mise --no-config --no-env --no-hooks install rust@1.98.1"),
        steps::run_step("Build versioned binary", &build_script(binary)),
        steps::run_step("Verify binary architecture", &architecture_script(binary, verify)),
        steps::run_step("Write SHA-256 sidecar", &checksum_script(binary, sidecar, target)),
        steps::upload_step("upload", &upload_name, &[binary, sidecar]),
    ];
    let fields = steps::build_fields(name, runs_on, &upload_name);
    steps::finish_job(if target == "linux" { "build-linux" } else { "build-macos" }, fields, job_steps)
}

pub(super) fn linux_attest_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    attest_job("linux", runs_on, assets)
}

pub(super) fn macos_attest_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    attest_job("macos", runs_on, assets)
}

fn attest_job(target: &str, runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    let (binary, sidecar, source_job, bundle) = if target == "linux" {
        (&assets.linux_binary, &assets.linux_sidecar, "build-linux", &assets.linux_bundle)
    } else {
        (&assets.macos_binary, &assets.macos_sidecar, "build-macos", &assets.macos_bundle)
    };
    let upload_name = format!("generator-{target}-provenance-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}");
    let fields = steps::attest_fields(
        format!("Attest {target} generator assets"),
        runs_on,
        source_job,
        &upload_name,
    );
    let job_steps = vec![
        steps::download_step(
            "Download exact build artifact",
            format!("${{{{ needs.{source_job}.outputs.artifact_id }}}}"),
            "assets",
        ),
        steps::attest_step(&format!("assets/{binary}\nassets/{sidecar}")),
        steps::copy_bundle_step(bundle),
        steps::upload_step("upload-bundle", &upload_name, &[bundle]),
    ];
    steps::finish_job(
        if target == "linux" { "attest-linux" } else { "attest-macos" },
        fields,
        job_steps,
    )
}

pub(super) fn manifest_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    let artifact_name = format!("generator-manifest-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}");
    let fields = steps::manifest_fields(&artifact_name);
    let steps = vec![
        steps::checkout_step(),
        steps::download_step(
            "Download exact Linux build artifact",
            "${{ needs.build-linux.outputs.artifact_id }}",
            "linux-assets",
        ),
        steps::download_step(
            "Download exact macOS build artifact",
            "${{ needs.build-macos.outputs.artifact_id }}",
            "macos-assets",
        ),
        steps::manifest_step(&assets.version),
        steps::upload_step("upload-manifest", &artifact_name, &[&assets.manifest]),
    ];
    steps::finish_job("build-manifest", steps::with_runner(fields, runs_on), steps)
}

pub(super) fn manifest_attest_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    let artifact_name = format!("generator-manifest-provenance-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}");
    let fields = steps::manifest_attest_fields(&artifact_name);
    let sequence = vec![
        steps::download_step(
            "Download exact manifest artifact",
            "${{ needs.build-manifest.outputs.artifact_id }}",
            "assets",
        ),
        steps::attest_step(&format!("assets/{}", assets.manifest)),
        steps::copy_bundle_step(&assets.manifest_bundle),
        steps::upload_step("upload-bundle", &artifact_name, &[&assets.manifest_bundle]),
    ];
    steps::finish_job("attest-manifest", steps::with_runner(fields, runs_on), sequence)
}

pub(super) fn publish_job(runs_on: Yaml, assets: &ReleaseAssets) -> (String, Yaml) {
    let needs = [
        "release-eligibility",
        "build-linux",
        "attest-linux",
        "build-macos",
        "attest-macos",
        "build-manifest",
        "attest-manifest",
    ];
    let fields = steps::publish_fields(runs_on, &needs, assets);
    let artifact_ids = steps::publish_artifact_ids();
    let sequence = vec![
        steps::checkout_step(),
        steps::download_all_step(&artifact_ids),
        steps::mise_step(),
        steps::run_step("Install pinned GitHub CLI", "mise --no-config --no-env --no-hooks install gh@2.102.0"),
        steps::run_step("Verify pinned GitHub CLI", "mise --no-config --no-env --no-hooks exec gh@2.102.0 -- gh --version"),
        steps::publish_step(&assets.version),
    ];
    steps::finish_job("publish-generator", fields, sequence)
}

fn build_script(binary: &str) -> String {
    format!(
        "set -euo pipefail\nmise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --release -p velnor-actions-cli\ncp target/release/velnor-actions {binary}\ntest -s {binary}"
    )
}

fn architecture_script(binary: &str, pattern: &str) -> String {
    format!(
        "set -euo pipefail\ndescription=\"$(file -b -- {binary})\"\ncase \"$description\" in\n  *{pattern}*) ;;\n  *) printf 'unexpected binary architecture: %s\\n' \"$description\" >&2; exit 1 ;;\nesac"
    )
}

fn checksum_script(binary: &str, sidecar: &str, target: &str) -> String {
    let command = if target == "linux" {
        "sha256sum"
    } else {
        "shasum -a 256"
    };
    format!("set -euo pipefail\n{command} {binary} > {sidecar}\ntest -s {sidecar}")
}
