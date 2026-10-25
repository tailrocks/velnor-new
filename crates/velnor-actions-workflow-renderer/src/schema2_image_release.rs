use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::features::run_step;
use super::release::{self, Publish};
use super::{Schema2WorkflowRequest, resource_probe_image};

const RUNNER_TAR: &str = "velnor-runner-linux-amd64.tar";
const DIND_TAR: &str = "velnor-dind-linux-amd64.tar";
const RESOURCE_PROBE_TAR: &str = "velnor-resource-probe-linux-amd64.tar";
const RESOURCE_PROBE_MANIFEST: &str = "RESOURCE_PROBE_MANIFEST.json";

const PROBE_SOURCE_REVISION: &str = "\
set -eu
source_revision=\"${GITHUB_SHA:?missing source revision}\"
[[ \"$source_revision\" =~ ^[0-9a-f]{40}$ ]]
printf 'VELNOR_SOURCE_REVISION=%s\\n' \"$source_revision\" >> \"$GITHUB_ENV\"";

const IMAGE_BUILD: &str = "bash images/resource-probe/build-runner-images.sh";

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
sha256sum velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar velnor-resource-probe-linux-amd64.tar RESOURCE_PROBE_MANIFEST.json > SHA256SUMS";

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
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
        release::CHECKSUMS,
    ];
    let mut build_steps = vec![run_step(
        "Bind resource probe to the release source",
        PROBE_SOURCE_REVISION,
    )];
    build_steps.extend(resource_probe_image::build_steps(pins)?);
    build_steps.push(run_step("Build Runner and DinD images", IMAGE_BUILD));
    build_steps.extend([
        run_step("Verify image architectures", IMAGE_VERIFY),
        run_step("Save image archives", IMAGE_SAVE),
        run_step("Checksum built bytes", IMAGE_SUM),
    ]);
    Ok(release::document(
        "Image release",
        vec![
            release::build_job(
                "build-images",
                "Build runner images",
                hosted.clone(),
                JobTimeout::PRODUCT_COMPILER,
                build_steps,
                "Upload image assets",
                &files,
            ),
            release::attest_job(
                "attest-images",
                "Attest runner images",
                hosted.clone(),
                "build-images",
                "image-assets",
                &files,
            ),
            release::publish_job(
                hosted,
                &Publish {
                    id: "publish-images",
                    name: "Publish runner images",
                    needs: "attest-images",
                    artifact: "image-assets",
                    prefix: "runner",
                    notes: "Runner image assets built from ${GITHUB_SHA}.",
                    files: &files,
                },
            ),
        ],
    ))
}
