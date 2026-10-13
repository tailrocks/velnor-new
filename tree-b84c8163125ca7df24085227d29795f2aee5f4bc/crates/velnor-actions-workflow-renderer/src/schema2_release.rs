//! Hosted image and macOS binary release workflows.
//!
//! `id-token` covers the whole job, so attest and GitHub-release upload are
//! different jobs. The attest job never receives `contents: write` or
//! `packages: write`. Checksums are `sha256sum` / `shasum` of the bytes just
//! built. Dispatch has no inputs, so a caller cannot supply a shell fragment
//! or a checksum.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;
use super::features::{CHECKOUT_USES, base, finish, publish_step, run_step};
use super::release_eligibility;

/// GitHub-hosted macOS label. The binary is native; it is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
/// Same `jdx/mise-action` commit CI pins. Not a floating tag.
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
/// Catalog version. The Linux cache checksum is not reused on macOS.
const MISE_VERSION: &str = "2026.9.18";
/// `actions/attest-build-provenance` tag `v4.2.2` (commit, not a floating tag).
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const ASSET_DIR: &str = "assets";
const CHECKSUMS: &str = "SHA256SUMS";
const RUNNER_TAR: &str = "velnor-runner-linux-amd64.tar";
const DIND_TAR: &str = "velnor-dind-linux-amd64.tar";
const HOST_BIN: &str = "velnor-host";

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
test -s velnor-dind-linux-amd64.tar";

const IMAGE_SUM: &str = "\
set -eu
sha256sum velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar > SHA256SUMS";

const RUST_INSTALL: &str = "\
set -eu
mise --no-config --no-env --no-hooks install rust@1.98.1";

const BINARY_BUILD: &str = "\
set -eu
mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --release -p velnor-runner-cli
cp crates/velnor-runner/target/release/velnor-host velnor-host
test -s velnor-host";

const BINARY_VERIFY: &str = "\
set -eu
desc=\"$(file -b velnor-host)\"
case \"$desc\" in
  *Mach-O*arm64*) ;;
  *) echo \"not an arm64 Mach-O: $desc\" >&2; exit 1 ;;
esac";

const BINARY_SUM: &str = "\
set -eu
shasum -a 256 velnor-host > SHA256SUMS";

/// Image release: build both linux/amd64 images, attest, then upload assets.
///
/// # Errors
///
/// An illegal hosted label fails.
pub(super) fn image_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let files = [RUNNER_TAR, DIND_TAR, CHECKSUMS];
    Ok(document(
        "Image release",
        vec![
            release_eligibility::job(hosted.clone(), super::IMAGE_RELEASE_WORKFLOW),
            build_job(
                "build-images",
                "Build runner images",
                hosted.clone(),
                60,
                super::IMAGE_RELEASE_WORKFLOW,
                vec![
                    run_step("Build images", IMAGE_BUILD),
                    run_step("Verify image architecture", IMAGE_VERIFY),
                    run_step("Save image tars", IMAGE_SAVE),
                    run_step("Checksum built bytes", IMAGE_SUM),
                ],
                "Upload image assets",
                &files,
            ),
            attest_job(
                "attest-images",
                "Attest runner images",
                hosted.clone(),
                "build-images",
                "image-assets",
                &files,
            ),
            publish_job(
                hosted,
                &Publish {
                    id: "publish-images",
                    name: "Publish runner images",
                    needs: "attest-images",
                    artifact: "image-assets",
                    prefix: "runner",
                    notes: "Runner image assets built from ${GITHUB_SHA}.",
                    files: &files,
                    workflow_path: super::IMAGE_RELEASE_WORKFLOW,
                    needs: &["attest-images", release_eligibility::JOB_ID],
                },
            ),
        ],
    ))
}

/// macOS binary release. Every job uses `macos-15`, never the Ubuntu label.
///
/// # Errors
///
/// An illegal macOS label fails.
pub(super) fn macos_binary_release(_request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let files = [HOST_BIN, CHECKSUMS];
    Ok(document(
        "macOS binary release",
        vec![
            release_eligibility::job(macos.clone(), super::MACOS_BINARY_RELEASE_WORKFLOW),
            build_job(
                "build-binary",
                "Build velnor-host",
                macos.clone(),
                120,
                super::MACOS_BINARY_RELEASE_WORKFLOW,
                vec![
                    mise_step(),
                    run_step("Install pinned Rust", RUST_INSTALL),
                    run_step("Build velnor-host", BINARY_BUILD),
                    run_step("Verify Mach-O architecture", BINARY_VERIFY),
                    run_step("Checksum built bytes", BINARY_SUM),
                ],
                "Upload binary asset",
                &files,
            ),
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
                    workflow_path: super::MACOS_BINARY_RELEASE_WORKFLOW,
                    needs: &["attest-binary", release_eligibility::JOB_ID],
                },
            ),
        ],
    ))
}

fn build_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    timeout: i64,
    workflow_path: &str,
    mut steps: Vec<Yaml>,
    upload_name: &str,
    files: &[&str],
) -> (String, Yaml) {
    let mut prefixed = vec![checkout_step(
        "${{ needs['release-eligibility'].outputs.source_sha }}",
    )];
    prefixed.append(&mut steps);
    prefixed.push(upload_step(upload_name, artifact_name(id), files));
    finish(
        id,
        with_needs(
            with_permissions(base(name, runs_on, timeout), build_permissions()),
            &[release_eligibility::JOB_ID],
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
            with_permissions(base(name, runs_on, 20), attest_permissions()),
            needs,
        ),
        vec![download_step(artifact), attest_step(&subject_list(files))],
    )
}

struct Publish<'a> {
    id: &'a str,
    name: &'a str,
    needs: &'a [&'a str],
    artifact: &'a str,
    prefix: &'a str,
    notes: &'a str,
    files: &'a [&'a str],
    workflow_path: &'a str,
}

fn publish_job(runs_on: Yaml, spec: &Publish<'_>) -> (String, Yaml) {
    finish(
        spec.id,
        with_needs(
            with_permissions(base(spec.name, runs_on, 30), publish_permissions()),
            spec.needs,
        ),
        vec![
            checkout_step("${{ needs['release-eligibility'].outputs.source_sha }}"),
            release_eligibility::mise_step(),
            run_step(
                "Install pinned GitHub CLI",
                &format!(
                    "mise --no-config --no-env --no-hooks install gh@{}",
                    release_eligibility::GH_VERSION
                ),
            ),
            download_step(spec.artifact),
            release_eligibility::check_step(spec.workflow_path),
            publish_step(&release_command(spec.prefix, spec.notes, spec.files)),
        ],
    )
}

fn artifact_name(build_id: &str) -> &'static str {
    if build_id.ends_with("images") {
        "image-assets"
    } else {
        "binary-assets"
    }
}

fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Setup Mise")),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

fn checkout_step(ref_value: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str(ref_value)),
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
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
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

fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().map(|need| Yaml::str(*need)).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attest job: `id-token`, no `contents: write`. Not a registry upload.
fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

/// Only the release-upload job may write repository contents.
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
            Yaml::Map(vec![(
                "workflow_dispatch".to_owned(),
                Yaml::Map(vec![(
                    "inputs".to_owned(),
                    Yaml::Map(vec![(
                        "source_sha".to_owned(),
                        Yaml::Map(vec![
                            (
                                "description".to_owned(),
                                Yaml::str("Exact tested main commit to build and publish"),
                            ),
                            ("required".to_owned(), Yaml::Bool(true)),
                            ("type".to_owned(), Yaml::str("string")),
                        ]),
                    )]),
                )]),
            )]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}
