// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ProductReleasePins, RenderError, Yaml, action, generator_release, map, product_release_family,
    release_artifact_check, run, seq, steps, strings,
};

pub(super) fn release_job(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let mut list = generator_release::product_setup_steps(
        pins,
        velnor_actions_contract::ReleaseTarget::LinuxX86_64,
    )?;
    list.push(release_artifact_check());
    list.push(action(
        "Download exact release assets",
        steps::DOWNLOAD_ARTIFACT_USES,
        &[
            (
                "artifact-ids",
                "${{ needs.publish-images.outputs.artifact_id }}",
            ),
            ("path", "${{ runner.temp }}/runner-family-release/assets"),
        ],
    ));
    list.extend(publisher_steps(pins)?);
    Ok(map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(120)),
        ("needs", seq(&["publish-images", "attest-images"])),
        (
            "if",
            Yaml::str("${{ needs.publish-images.outputs.action == 'build' }}"),
        ),
        (
            "permissions",
            strings(&[
                ("actions", "read"),
                ("attestations", "read"),
                ("contents", "write"),
            ]),
        ),
        ("steps", Yaml::Seq(list)),
    ]))
}

fn publisher_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut list = Vec::new();
    let script = format!(
        "set -eu\nmkdir -m 0700 assets\ncp \"$RUNNER_TEMP/runner-family-release/assets/RUNNER_IMAGE_MANIFEST.json\" \"$RUNNER_TEMP/runner-family-release/assets/SHA256SUMS\" \"$RUNNER_TEMP/runner-family-release/assets/LICENSE\" \"$RUNNER_TEMP/runner-family-release/assets/NOTICE\" assets/\n{}",
        product_release_family::runner_publish_script(pins)?
    );
    list.extend(super::authority::shell_steps(
        &script,
        "publish.sh",
        "Materialize fixed runner source release publication",
    ));
    list.push(run(
        "Publish immutable source release",
        "set -euo pipefail\nbash \"$VELNOR_RUNNER_RELEASE_AUTHORITY_DIR/publish.sh\"".to_owned(),
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("VELNOR_SOURCE_SHA", "${{ github.sha }}"),
            (
                "VELNOR_WORKFLOW_AUTHORITY_SHA",
                "${{ github.workflow_sha }}",
            ),
            (
                "VELNOR_CI_RUN_ID",
                "${{ needs.publish-images.outputs.producer_run_id }}",
            ),
            (
                "VELNOR_CI_ATTEMPT",
                "${{ needs.publish-images.outputs.producer_run_attempt }}",
            ),
            ("VELNOR_RELEASE_ACTION", "build"),
            (
                "VELNOR_MANIFEST_DIGEST",
                "${{ needs.attest-images.outputs.manifest_digest }}",
            ),
            (
                "VELNOR_CHECKSUMS_DIGEST",
                "${{ needs.attest-images.outputs.checksums_digest }}",
            ),
            (
                "VELNOR_LICENSE_DIGEST",
                "${{ needs.attest-images.outputs.license_digest }}",
            ),
            (
                "VELNOR_NOTICE_DIGEST",
                "${{ needs.attest-images.outputs.notice_digest }}",
            ),
            (
                "VELNOR_FAMILY_ASSETS",
                "${{ runner.temp }}/runner-family-release/assets",
            ),
        ],
    ));
    Ok(list)
}
