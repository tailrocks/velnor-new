// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, steps, yaml::Yaml};
use velnor_actions_contract::ReleaseTarget;

use super::{
    ProductReleasePins, action, generator_release, map, release_artifact_check, run, seq, strings,
};

pub(super) fn preview_call() -> Result<Yaml, RenderError> {
    let sha = velnor_actions_contract::source_preview::RUNTIME_AUTHORITY_SHA.ok_or_else(|| {
        RenderError::InvalidWorkflow("preview_runtime_authority_unavailable".to_owned())
    })?;
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_runtime_authority_invalid".to_owned(),
        ));
    }
    Ok(map(&[
        ("needs", seq(&["discover-producer", "publish-images"])),
        (
            "if",
            Yaml::str("${{ needs.publish-images.outputs.action == 'build' }}"),
        ),
        (
            "uses",
            Yaml::uses(format!(
                "tailrocks/velnor-new/.github/workflows/source-preview-mac-proof.yml@{sha}"
            )),
        ),
        (
            "permissions",
            strings(&[("actions", "read"), ("contents", "read")]),
        ),
        (
            "with",
            strings(&[
                ("public_source", "${{ github.sha }}"),
                ("public_run_id", "${{ github.run_id }}"),
                ("public_run_attempt", "${{ github.run_attempt }}"),
                (
                    "producer_run_id",
                    "${{ needs.discover-producer.outputs.producer_run_id }}",
                ),
                (
                    "producer_run_attempt",
                    "${{ needs.discover-producer.outputs.producer_run_attempt }}",
                ),
                (
                    "producer_artifact_id",
                    "${{ needs.discover-producer.outputs.producer_artifact_id }}",
                ),
                (
                    "publisher_artifact_id",
                    "${{ needs.publish-images.outputs.artifact_id }}",
                ),
            ]),
        ),
    ]))
}

pub(super) fn verify_job(
    pins: &ProductReleasePins,
    mac: &velnor_actions_contract::config::MacRunnerQualificationConfig,
) -> Result<Yaml, RenderError> {
    let steps = verify_steps(pins, mac)?;
    Ok(map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(140)),
        ("needs", seq(&["discover-producer", "publish-images"])),
        (
            "outputs",
            map(&[
                (
                    "manifest_digest",
                    Yaml::str("${{ steps.verify_mac.outputs.manifest_digest }}"),
                ),
                (
                    "checksums_digest",
                    Yaml::str("${{ steps.verify_mac.outputs.checksums_digest }}"),
                ),
                (
                    "license_digest",
                    Yaml::str("${{ steps.verify_mac.outputs.license_digest }}"),
                ),
                (
                    "notice_digest",
                    Yaml::str("${{ steps.verify_mac.outputs.notice_digest }}"),
                ),
            ]),
        ),
        (
            "if",
            Yaml::str("${{ needs.publish-images.outputs.action == 'build' }}"),
        ),
        (
            "permissions",
            strings(&[("actions", "read"), ("contents", "read")]),
        ),
        (
            "steps",
            Yaml::Seq(crate::schema2::features::guard_artifact_downloads(steps)),
        ),
    ]))
}

fn verify_steps(
    pins: &ProductReleasePins,
    mac: &velnor_actions_contract::config::MacRunnerQualificationConfig,
) -> Result<Vec<Yaml>, RenderError> {
    let download = action(
        "Download exact release assets",
        steps::DOWNLOAD_ARTIFACT_USES,
        &[
            (
                "artifact-ids",
                "${{ needs.publish-images.outputs.artifact_id }}",
            ),
            ("path", "${{ runner.temp }}/runner-family-release/assets"),
        ],
    );
    let mut steps = generator_release::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    steps.push(release_artifact_check());
    steps.push(download);
    let velnor_actions_contract::config::MacSdkSelection::Stable {
        version,
        source_sha,
        manifest_sha256,
    } = &mac.sdk
    else {
        return Err(RenderError::InvalidWorkflow(
            "source_preview_runtime_bootstrap_unavailable".to_owned(),
        ));
    };
    let gh = crate::commands::join_argv_for_run(&pins.gh_argv)?;
    let bootstrap = crate::homebrew_adoption_workflow::bootstrap::bootstrap_script(
        &crate::homebrew_adoption_workflow::bootstrap::BootstrapEnv {
            version: "VELNOR_SDK_VERSION",
            source_sha: "VELNOR_SDK_SOURCE_SHA",
            manifest_sha256: "VELNOR_SDK_MANIFEST_SHA256",
            gh: "VELNOR_SDK_GH",
        },
    );
    steps.push(run(
        "Authenticate and stage published SDK",
        bootstrap,
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("VELNOR_SDK_GH", &gh),
            ("VELNOR_SDK_VERSION", version),
            ("VELNOR_SDK_SOURCE_SHA", source_sha),
            ("VELNOR_SDK_MANIFEST_SHA256", manifest_sha256),
        ],
    ));
    let mut verify = run("Verify production Mac qualification", concat!(
        "set -euo pipefail\n",
        "\"$RUNNER_TEMP/velnor/bin/velnor-actions-$VELNOR_SDK_VERSION\" image-maintenance verify-mac-attestation ",
        "--producer-run-id \"$PRODUCER_RUN_ID\" --producer-run-attempt \"$PRODUCER_RUN_ATTEMPT\" ",
        "--producer-artifact-id \"$PRODUCER_ARTIFACT_ID\" --publisher-artifact-id \"$PUBLISHER_ARTIFACT_ID\" ",
        "--assets-dir \"$RUNNER_TEMP/runner-family-release/assets\" --runner-temp \"$RUNNER_TEMP\""
    ).to_owned(), &[
        ("GH_TOKEN", "${{ github.token }}"),
        ("VELNOR_SDK_VERSION", version),
        ("PRODUCER_RUN_ID", "${{ needs.discover-producer.outputs.producer_run_id }}"),
        ("PRODUCER_RUN_ATTEMPT", "${{ needs.discover-producer.outputs.producer_run_attempt }}"),
        ("PRODUCER_ARTIFACT_ID", "${{ needs.discover-producer.outputs.producer_artifact_id }}"),
        ("PUBLISHER_ARTIFACT_ID", "${{ needs.publish-images.outputs.artifact_id }}"),
    ]);
    if let Yaml::Map(fields) = &mut verify {
        fields.push(("id".into(), Yaml::str("verify_mac")));
    }
    steps.push(verify);
    Ok(steps)
}

pub(super) fn attest_job() -> Yaml {
    let names = [
        ("RUNNER_IMAGE_MANIFEST.json", "manifest_digest"),
        ("SHA256SUMS", "checksums_digest"),
        ("LICENSE", "license_digest"),
        ("NOTICE", "notice_digest"),
    ];
    let outputs = Yaml::Map(
        names
            .iter()
            .map(|(_, key)| {
                (
                    (*key).to_owned(),
                    Yaml::str(format!("${{{{ needs.verify-mac-proof.outputs.{key} }}}}")),
                )
            })
            .collect(),
    );
    let attestations = names
        .iter()
        .map(|(name, key)| {
            let digest = format!("${{{{ needs.verify-mac-proof.outputs.{key} }}}}");
            action(
                "Attest verified release asset",
                steps::ATTEST_BUILD_PROVENANCE_USES,
                &[("subject-name", name), ("subject-digest", &digest)],
            )
        })
        .collect();
    map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(10)),
        ("needs", seq(&["verify-mac-proof"])),
        ("outputs", outputs),
        (
            "permissions",
            strings(&[
                ("attestations", "write"),
                ("contents", "read"),
                ("id-token", "write"),
            ]),
        ),
        ("steps", Yaml::Seq(attestations)),
    ])
}
