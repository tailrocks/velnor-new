// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, steps, yaml::Yaml};
use velnor_actions_contract::ReleaseTarget;
#[path = "schema2_runner_image_release/attest.rs"]
mod attest;
#[path = "schema2_runner_image_release/authority.rs"]
mod authority;
#[path = "schema2_runner_image_release/release.rs"]
mod release;

use super::super::{
    ProductReleasePins, Schema2WorkflowRequest, generator_release, product_release_family,
};

const PUBLICATION_ALLOWANCE_MINUTES: i64 =
    velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_PUBLICATION_JOB_TIMEOUT_MINUTES as i64;
const DISCOVERY_MINUTES: i64 = 350;

fn map(items: &[(&str, Yaml)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

fn seq(items: &[&str]) -> Yaml {
    Yaml::Seq(items.iter().map(|item| Yaml::str(*item)).collect())
}

fn run(name: &str, script: String, env: &[(&str, &str)]) -> Yaml {
    map(&[
        ("name", Yaml::str(name)),
        ("shell", Yaml::str("bash")),
        ("env", strings(env)),
        ("run", Yaml::str(script)),
    ])
}

fn strings(items: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn action(name: &str, uses: &str, with: &[(&str, &str)]) -> Yaml {
    map(&[
        ("name", Yaml::str(name)),
        ("uses", Yaml::str(uses)),
        ("with", strings(with)),
    ])
}

fn release_artifact_check() -> Yaml {
    run(
        "Validate release artifact identity",
        "set -eu\nexport LC_ALL=C\ncase \"$ARTIFACT_ID\" in ''|*[!0-9]*|0|0*) exit 1;; esac\nlength=\"${#ARTIFACT_ID}\"\n[ \"$length\" -lt 16 ] || { [ \"$length\" -eq 16 ] && [ \"$ARTIFACT_ID\" -le 9007199254740991 ]; }".to_owned(),
        &[("ARTIFACT_ID", "${{ needs.publish-images.outputs.artifact_id }}")],
    )
}

fn tools(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut result = generator_release::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    result.extend(authority::materialize_steps());
    Ok(result)
}

fn discover(pins: &ProductReleasePins, phase: &str) -> Result<Yaml, RenderError> {
    let timeout = (DISCOVERY_MINUTES * 60).to_string();
    let mut step = run(
        "Discover exact producer and qualification artifacts",
        format!(
            "{}\n{}\n{}",
            generator_release::gh_function(pins)?,
            include_str!("../../../scripts/runner-family-discovery.sh"),
            include_str!("../../../scripts/resolve-runner-family-publication.sh").replace(
                "source \"$(dirname \"${BASH_SOURCE[0]}\")/runner-family-discovery.sh\"",
                "",
            ),
        ),
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS", &timeout),
            ("VELNOR_FAMILY_DISCOVERY_PHASE", phase),
        ],
    );
    if let Yaml::Map(fields) = &mut step {
        fields.push(("id".into(), Yaml::str("discovery")));
    }
    Ok(step)
}

fn downloads() -> Vec<Yaml> {
    vec![
        action(
            "Download exact producer artifact",
            steps::DOWNLOAD_ARTIFACT_USES,
            &[
                (
                    "artifact-ids",
                    "${{ needs.discover-producer.outputs.producer_artifact_id }}",
                ),
                (
                    "run-id",
                    "${{ needs.discover-producer.outputs.producer_run_id }}",
                ),
                ("github-token", "${{ github.token }}"),
                ("path", "${{ runner.temp }}/runner-family-candidate"),
            ],
        ),
        action(
            "Download signed qualification receipt",
            steps::DOWNLOAD_ARTIFACT_USES,
            &[
                (
                    "artifact-ids",
                    "${{ needs.discover-artifacts.outputs.qualification_artifact_id }}",
                ),
                (
                    "run-id",
                    "${{ needs.discover-artifacts.outputs.qualifier_run_id }}",
                ),
                ("github-token", "${{ github.token }}"),
                ("path", "${{ runner.temp }}/runner-family-qualification"),
            ],
        ),
    ]
}

fn publisher_env() -> [(&'static str, &'static str); 7] {
    [
        ("GH_TOKEN", "${{ github.token }}"),
        (
            "PRODUCER_RUN_ID",
            "${{ needs.discover-producer.outputs.producer_run_id }}",
        ),
        (
            "PRODUCER_RUN_ATTEMPT",
            "${{ needs.discover-producer.outputs.producer_run_attempt }}",
        ),
        (
            "PRODUCER_ARTIFACT_ID",
            "${{ needs.discover-producer.outputs.producer_artifact_id }}",
        ),
        (
            "QUALIFIER_RUN_ID",
            "${{ needs.discover-artifacts.outputs.qualifier_run_id }}",
        ),
        (
            "QUALIFIER_RUN_ATTEMPT",
            "${{ needs.discover-artifacts.outputs.qualifier_run_attempt }}",
        ),
        (
            "QUALIFIER_JOB_ID",
            "${{ needs.discover-artifacts.outputs.qualifier_job_id }}",
        ),
    ]
}

#[path = "schema2_runner_image_release/context.rs"]
mod context;

fn publish_job(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let mut list = tools(pins)?;
    list.extend(downloads());
    list.push(authority::verification_step(pins)?);
    list.push(run(
        "Publish authenticated runner family",
        "set -euo pipefail\npython3 \"$VELNOR_RUNNER_AUTHORITY_DIR/runner_image_authority_driver.py\"".to_owned(),
        &publisher_env(),
    ));
    let mut prepare = run(
        "Reconcile immutable release",
        product_release_family::runner_prepare_script(pins)?,
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("VELNOR_SOURCE_SHA", "${{ github.sha }}"),
            (
                "VELNOR_WORKFLOW_AUTHORITY_SHA",
                "${{ github.workflow_sha }}",
            ),
            (
                "VELNOR_FAMILY_ASSETS",
                "${{ runner.temp }}/runner-family-release/assets",
            ),
        ],
    );
    if let Yaml::Map(fields) = &mut prepare {
        fields.push(("id".into(), Yaml::str("reconcile")));
    }
    list.push(prepare);
    let mut upload = action(
        "Upload exact release assets",
        steps::UPLOAD_ARTIFACT_USES,
        &[
            ("name", "runner-image-release-${{ github.run_attempt }}"),
            ("path", "${{ runner.temp }}/runner-family-release/assets"),
            ("if-no-files-found", "error"),
        ],
    );
    if let Yaml::Map(fields) = &mut upload {
        fields.push(("id".into(), Yaml::str("assets")));
        fields.push((
            "if".into(),
            Yaml::str("${{ steps.reconcile.outputs.action == 'build' }}"),
        ));
    }
    list.push(upload);
    list.extend(context::steps());
    Ok(map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(PUBLICATION_ALLOWANCE_MINUTES)),
        (
            "permissions",
            strings(&[
                ("actions", "read"),
                ("attestations", "read"),
                ("contents", "read"),
                ("packages", "write"),
            ]),
        ),
        ("needs", seq(&["discover-producer", "discover-artifacts"])),
        ("outputs", publish_outputs()),
        ("steps", Yaml::Seq(list)),
    ]))
}

fn publish_outputs() -> Yaml {
    map(&[
        ("action", Yaml::str("${{ steps.reconcile.outputs.action }}")),
        (
            "context_artifact_id",
            Yaml::str("${{ steps.context.outputs.artifact-id }}"),
        ),
        (
            "context_artifact_digest",
            Yaml::str("${{ steps.context.outputs.artifact-digest }}"),
        ),
        (
            "artifact_id",
            Yaml::str("${{ steps.assets.outputs.artifact-id }}"),
        ),
        (
            "producer_run_id",
            Yaml::str("${{ needs.discover-producer.outputs.producer_run_id }}"),
        ),
        (
            "producer_run_attempt",
            Yaml::str("${{ needs.discover-producer.outputs.producer_run_attempt }}"),
        ),
    ])
}

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = release_pins(request)?;
    Ok(map(&[
        ("name", Yaml::str("Runner image release")),
        ("on", map(&[("push", map(&[("branches", seq(&["main"]))]))])),
        (
            "concurrency",
            map(&[
                (
                    "group",
                    Yaml::str("${{ github.repository }}-product-release"),
                ),
                ("cancel-in-progress", Yaml::Bool(false)),
            ]),
        ),
        ("permissions", strings(&[("contents", "read")])),
        (
            "jobs",
            map(&[
                ("discover-producer", discovery_job(pins, "producer", false)?),
                (
                    "discover-artifacts",
                    discovery_job(pins, "qualification", true)?,
                ),
                ("publish-images", publish_job(pins)?),
                (
                    "verify-mac-proof",
                    match &request
                        .runner_image_family
                        .as_ref()
                        .ok_or_else(|| {
                            RenderError::InvalidWorkflow(
                                "mac_qualification_authority_unavailable".into(),
                            )
                        })?
                        .sdk
                    {
                        velnor_actions_contract::config::MacSdkSelection::Stable { .. } => {
                            attest::verify_job(
                                pins,
                                request.runner_image_family.as_ref().ok_or_else(|| {
                                    RenderError::InvalidWorkflow(
                                        "mac_qualification_authority_unavailable".into(),
                                    )
                                })?,
                            )?
                        }
                        velnor_actions_contract::config::MacSdkSelection::SourcePreview {} => {
                            attest::preview_call()?
                        }
                    },
                ),
                ("attest-images", attest::attest_job()),
                ("release-images", release::release_job(pins)?),
            ]),
        ),
    ]))
}

fn discovery_job(
    pins: &ProductReleasePins,
    phase: &str,
    depends: bool,
) -> Result<Yaml, RenderError> {
    let mut discovery_steps =
        generator_release::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    discovery_steps.push(discover(pins, phase)?);
    let mut discovery = map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(360)),
        (
            "permissions",
            strings(&[("actions", "read"), ("contents", "read")]),
        ),
        (
            "outputs",
            map(&[
                (
                    "producer_run_id",
                    Yaml::str("${{ steps.discovery.outputs.producer_run_id }}"),
                ),
                (
                    "producer_run_attempt",
                    Yaml::str("${{ steps.discovery.outputs.producer_run_attempt }}"),
                ),
                (
                    "producer_artifact_id",
                    Yaml::str("${{ steps.discovery.outputs.producer_artifact_id }}"),
                ),
                (
                    "qualifier_run_id",
                    Yaml::str("${{ steps.discovery.outputs.qualifier_run_id }}"),
                ),
                (
                    "qualifier_run_attempt",
                    Yaml::str("${{ steps.discovery.outputs.qualifier_run_attempt }}"),
                ),
                (
                    "qualifier_job_id",
                    Yaml::str("${{ steps.discovery.outputs.qualifier_job_id }}"),
                ),
                (
                    "qualification_artifact_id",
                    Yaml::str("${{ steps.discovery.outputs.qualification_artifact_id }}"),
                ),
            ]),
        ),
        ("steps", Yaml::Seq(discovery_steps)),
    ]);
    if depends && let Yaml::Map(fields) = &mut discovery {
        fields.push(("needs".into(), seq(&["discover-producer"])));
    }
    Ok(discovery)
}

fn release_pins(request: &Schema2WorkflowRequest) -> Result<&ProductReleasePins, RenderError> {
    let mac = request.runner_image_family.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("mac_qualification_authority_unavailable".into())
    })?;
    mac.validate("runner_image_family.mac_qualification")
        .map_err(RenderError::Contract)?;
    request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".into()))
}
