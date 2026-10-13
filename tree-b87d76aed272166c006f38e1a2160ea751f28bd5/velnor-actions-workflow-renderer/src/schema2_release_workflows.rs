//! Native product release workflow definitions.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;
use super::features::run_step;
use super::release::{Publish, attest_job, build_job, document, mise_step, publish_job};
use super::release_eligibility;

/// GitHub-hosted macOS label. The binary is native; it is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
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
                &["build-images"],
                "image-assets",
                &files,
            ),
            publish_job(
                hosted,
                &Publish {
                    id: "publish-images",
                    name: "Publish runner images",
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
                &["build-binary"],
                "binary-assets",
                &files,
            ),
            publish_job(
                macos,
                &Publish {
                    id: "publish-binary",
                    name: "Publish velnor-host",
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
