// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::commands::join_argv_for_run;
use crate::runs_on::runs_on_yaml;
use crate::steps::{ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::features::{CHECKOUT_USES, base, finish, identified_publish_step, run_step};
use super::{Schema2WorkflowRequest, generator_release, product_release_family};
use velnor_actions_contract::ReleaseTarget;

const MACOS_RUNS_ON: &str = "macos-15";
const ASSET_DIR: &str = "assets";
const CHECKSUMS: &str = "SHA256SUMS";
const RUNNER_TAR: &str = "velnor-runner-linux-amd64.tar";
const DIND_TAR: &str = "velnor-dind-linux-amd64.tar";
const RESOURCE_PROBE_TAR: &str = "velnor-resource-probe-linux-amd64.tar";
const RESOURCE_PROBE_MANIFEST: &str = "RESOURCE_PROBE_MANIFEST.json";
const IMAGE_ASSET_ARTIFACT: &str = "image-assets-${{ github.run_attempt }}";
const HOST_BIN: &str = "velnor-host";
const ATTESTATION_HELPER_BIN: &str = "velnor-runner-attestation-helper";
const LICENSE: &str = "LICENSE";
const NOTICE: &str = "NOTICE";

const IMAGE_BUILD: &str = "\
set -eu
docker build --platform linux/amd64 -t velnor-runner:linux-amd64 images/runner/ubuntu-26.04
docker build --platform linux/amd64 -t velnor-dind:linux-amd64 images/dind";

const IMAGE_VERIFY: &str = "\
set -eu
runner=\"$(docker image inspect --format '{{.Architecture}}' velnor-runner:linux-amd64)\"
dind=\"$(docker image inspect --format '{{.Architecture}}' velnor-dind:linux-amd64)\"
test \"$runner\" = amd64
test \"$dind\" = amd64";

const IMAGE_SAVE: &str = "\
set -eu
docker save --output velnor-runner-linux-amd64.tar velnor-runner:linux-amd64
docker save --output velnor-dind-linux-amd64.tar velnor-dind:linux-amd64
test -s velnor-runner-linux-amd64.tar
test -s velnor-dind-linux-amd64.tar
test -s velnor-resource-probe-linux-amd64.tar
test -s RESOURCE_PROBE_MANIFEST.json";

const IMAGE_SUM: &str = "\
set -eu
sha256sum velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar velnor-resource-probe-linux-amd64.tar RESOURCE_PROBE_MANIFEST.json LICENSE NOTICE > SHA256SUMS";

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

pub(super) fn image_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let files = [
        RUNNER_TAR,
        DIND_TAR,
        RESOURCE_PROBE_TAR,
        RESOURCE_PROBE_MANIFEST,
        LICENSE,
        NOTICE,
        CHECKSUMS,
    ];
    let mut build_steps = vec![run_step("Build runner and DinD images", IMAGE_BUILD)];
    build_steps.extend(super::resource_probe_image::build_steps(pins)?);
    build_steps.extend([
        run_step("Verify image architectures", IMAGE_VERIFY),
        run_step("Save image archives", IMAGE_SAVE),
        run_step("Checksum built bytes", IMAGE_SUM),
    ]);
    Ok(document(
        "Image release",
        vec![
            build_job(BuildJob {
                id: "build-images",
                name: "Build runner images",
                runs_on: hosted.clone(),
                timeout: JobTimeout::PRODUCT_IMAGE_BUILD,
                steps: build_steps,
                upload_name: "Upload image assets",
                artifact: IMAGE_ASSET_ARTIFACT,
                files: &files,
            }),
            attest_job(
                "attest-images",
                "Attest runner images",
                hosted.clone(),
                "build-images",
                IMAGE_ASSET_ARTIFACT,
                &files,
            ),
            publish_job(
                hosted,
                &Publish {
                    id: "publish-images",
                    name: "Publish runner images",
                    needs: "attest-images",
                    artifact: IMAGE_ASSET_ARTIFACT,
                    prefix: "runner",
                    notes: "Runner image assets built from ${GITHUB_SHA}.",
                    files: &files,
                },
            ),
        ],
    ))
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
                artifact: "binary-assets",
                files: &files,
            }),
            attest_job(
                "attest-binary",
                "Attest velnor-host",
                macos.clone(),
                "build-binary",
                "binary-assets",
                &files,
            ),
            publish_job(
                macos,
                &Publish {
                    id: "publish-binary",
                    name: "Publish velnor-host",
                    needs: "attest-binary",
                    artifact: "binary-assets",
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
    finish(
        spec.id,
        with_permissions(
            base(spec.name, spec.runs_on, spec.timeout),
            build_permissions(),
        ),
        prefixed,
    )
}

fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    needs: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    finish(
        id,
        with_needs(
            with_permissions(
                base(name, runs_on, JobTimeout::DEFAULT),
                attest_permissions(),
            ),
            needs,
        ),
        vec![download_step(artifact), attest_step(&subject_list(files))],
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
            download_step(spec.artifact),
            identified_publish_step(
                product_release_family::PUBLISH_STEP_ID,
                &release_command(spec.prefix, spec.notes, spec.files),
            ),
        ],
    )
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
            ]),
        ),
    ])
}

fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(newline_list(files))),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

fn download_step(artifact: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download built assets")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(ASSET_DIR)),
            ]),
        ),
    ])
}

fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

fn with_permissions(mut fields: Vec<(String, Yaml)>, perms: Yaml) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), perms));
    fields
}

fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &str) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(vec![Yaml::str(needs.to_owned())]),
    ));
    fields
}

fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

fn publish_permissions() -> Yaml {
    perm(&[("actions", "read"), ("contents", "write")])
}

fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn subject_list(files: &[&str]) -> String {
    files
        .iter()
        .map(|file| format!("{ASSET_DIR}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

fn release_command(prefix: &str, notes: &str, files: &[&str]) -> String {
    format!(
        "set -eu\ncd {ASSET_DIR}\ntag=\"{prefix}-${{GITHUB_SHA}}\"\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"{notes}\" {}",
        files.join(" ")
    )
}

fn document(name: &str, jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}
