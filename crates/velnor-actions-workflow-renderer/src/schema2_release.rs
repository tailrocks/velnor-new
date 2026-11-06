// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::commands::join_argv_for_run;
use crate::runs_on::runs_on_yaml;
use crate::steps::{ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::features::{CHECKOUT_USES, base, finish, identified_publish_step, run_step};
use super::{
    Schema2WorkflowRequest, generator_release, product_release_family,
    product_release_family::ARTIFACT_ID_GUARD_STEP_ID,
};
use velnor_actions_contract::ReleaseTarget;

#[path = "schema2_release_artifact_helpers.rs"]
mod artifact_helpers;
use artifact_helpers::{
    attest_permissions, attest_step, build_permissions, checkout_step, document,
    download_by_id_step, publish_permissions, release_command, subject_list, upload_step,
    validate_artifact_id_step, with_needs, with_permissions,
};

const MACOS_RUNS_ON: &str = "macos-15";
const ASSET_DIR: &str = "assets";
const CHECKSUMS: &str = "SHA256SUMS";
const BINARY_ASSET_ARTIFACT: &str = "binary-assets-${{ github.run_attempt }}";
const UPLOAD_ASSETS_STEP_ID: &str = "upload-assets";
const BINARY_ASSET_ARTIFACT_ID: &str = "${{ needs.build-binary.outputs.artifact_id }}";
const BINARY_ATTESTED_ARTIFACT_ID: &str = "${{ needs.attest-binary.outputs.artifact_id }}";
const HOST_BIN: &str = "velnor-host";
const ATTESTATION_HELPER_BIN: &str = "velnor-runner-attestation-helper";
const LICENSE: &str = "LICENSE";
const NOTICE: &str = "NOTICE";

const BINARY_VERIFY: &str = "\
set -eu
for binary in velnor-host velnor-runner-attestation-helper; do
  desc=\"$(file -b \"$binary\")\"
  case \"$desc\" in
    *Mach-O*arm64*) ;;
    *) echo \"The binary is not an arm64 Mach-O file: $binary: $desc.\" >&2; exit 1 ;;
  esac
done";

const BINARY_SUM: &str = "\
set -eu
shasum -a 256 velnor-host velnor-runner-attestation-helper LICENSE NOTICE > SHA256SUMS";

#[path = "schema2_runner_image_release.rs"]
mod runner_images;

pub(super) fn image_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    runner_images::render(request)
}

pub(super) fn image_release_caller(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    runner_images::caller(request)
}

pub(super) fn image_release_eligibility() -> String {
    runner_images::eligibility()
}

pub(super) fn macos_binary_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let install = join_argv_for_run(&pins.install_build_tools_argv)?;
    let build = format!(
        "set -euo pipefail\n: \"${{VELNOR_SOURCE_SHA:?}}\"\n{}\nhelper_path=crates/velnor-runner/target/release/velnor-runner-attestation-helper\ntest -s \"$helper_path\"\nhelper_sha=\"$(shasum -a 256 \"$helper_path\" | awk 'NR == 1 {{ print $1 }}')\"\n[[ \"$helper_sha\" =~ ^[0-9a-f]{{64}}$ ]]\nenv VELNOR_RUNNER_BUILD_SOURCE_SHA=\"$VELNOR_SOURCE_SHA\" VELNOR_RUNNER_BUILD_HELPER_SHA256=\"$helper_sha\" {}\ncp crates/velnor-runner/target/release/velnor-host velnor-host\ncp \"$helper_path\" velnor-runner-attestation-helper\ntest -s velnor-host\ntest -s velnor-runner-attestation-helper\ncopied_helper_sha=\"$(shasum -a 256 velnor-runner-attestation-helper | awk 'NR == 1 {{ print $1 }}')\"\ntest \"$copied_helper_sha\" = \"$helper_sha\"",
        join_argv_for_run(&pins.attestation_helper_build_argv)?,
        join_argv_for_run(&pins.runner_build_argv)?
    );
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let files = [HOST_BIN, ATTESTATION_HELPER_BIN, LICENSE, NOTICE, CHECKSUMS];
    Ok(document(
        "macOS binary release",
        vec![
            build_job(BuildJob {
                id: "build-binary",
                name: "Build velnor-host",
                runs_on: macos.clone(),
                timeout: JobTimeout::PRODUCT_COMPILER,
                steps: vec![
                    generator_release::mise_setup_step(pins, ReleaseTarget::MacosArm64)?,
                    run_step("Install pinned Rust and MBX", &install),
                    generator_release::verify_rust_step(pins)?,
                    run_step("Build velnor-host with MBX", &build),
                    run_step("Verify Mach-O architecture", BINARY_VERIFY),
                    run_step("Checksum built bytes", BINARY_SUM),
                ],
                upload_name: "Upload binary asset",
                artifact: BINARY_ASSET_ARTIFACT,
                files: &files,
            }),
            attest_job(
                "attest-binary",
                "Attest velnor-host",
                macos.clone(),
                "build-binary",
                BINARY_ASSET_ARTIFACT_ID,
                &files,
            ),
            publish_job(
                macos,
                &Publish {
                    id: "publish-binary",
                    name: "Publish velnor-host",
                    needs: "attest-binary",
                    artifact: BINARY_ATTESTED_ARTIFACT_ID,
                    prefix: "binary",
                    notes: "velnor-host built from ${GITHUB_SHA}.",
                    files: &files,
                },
            ),
        ],
    ))
}

struct BuildJob<'a> {
    id: &'a str,
    name: &'a str,
    runs_on: Yaml,
    timeout: JobTimeout,
    steps: Vec<Yaml>,
    upload_name: &'a str,
    artifact: &'a str,
    files: &'a [&'a str],
}

fn build_job(spec: BuildJob<'_>) -> (String, Yaml) {
    let mut prefixed = vec![checkout_step()];
    prefixed.extend(spec.steps);
    prefixed.push(upload_step(spec.upload_name, spec.artifact, spec.files));
    let mut fields = with_permissions(
        base(spec.name, spec.runs_on, spec.timeout),
        build_permissions(),
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "artifact_id".to_owned(),
            Yaml::str(format!(
                "${{{{ steps.{UPLOAD_ASSETS_STEP_ID}.outputs.artifact-id }}}}"
            )),
        )]),
    ));
    finish(spec.id, fields, prefixed)
}

fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    needs: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    let mut fields = with_needs(
        with_permissions(
            base(name, runs_on, JobTimeout::DEFAULT),
            attest_permissions(),
        ),
        needs,
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![("artifact_id".to_owned(), Yaml::str(artifact))]),
    ));
    finish(
        id,
        fields,
        vec![
            validate_artifact_id_step(artifact),
            download_by_id_step(artifact),
            attest_step(&subject_list(files)),
        ],
    )
}

struct Publish<'a> {
    id: &'a str,
    name: &'a str,
    needs: &'a str,
    artifact: &'a str,
    prefix: &'a str,
    notes: &'a str,
    files: &'a [&'a str],
}

fn publish_job(runs_on: Yaml, spec: &Publish<'_>) -> (String, Yaml) {
    finish(
        spec.id,
        with_needs(
            with_permissions(
                base(spec.name, runs_on, JobTimeout::DEFAULT),
                publish_permissions(),
            ),
            spec.needs,
        ),
        vec![
            checkout_step(),
            validate_artifact_id_step(spec.artifact),
            download_by_id_step(spec.artifact),
            identified_publish_step(
                product_release_family::PUBLISH_STEP_ID,
                &release_command(spec.prefix, spec.notes, spec.files),
            ),
        ],
    )
}
