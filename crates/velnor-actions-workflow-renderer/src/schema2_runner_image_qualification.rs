// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{features::CHECKOUT_USES, generator_release};
use crate::{RenderError, Schema2WorkflowRequest, steps, yaml::Yaml};
use velnor_actions_contract::ReleaseTarget;

pub(super) const CALLER_PATH: &str = ".github/workflows/runner-image-qualification.yml";
pub(super) const SIGNER_PATH: &str = ".github/workflows/runner-image-qualification-attest.yml";
pub(super) const QUALIFIER_TIMEOUT_MINUTES: i64 = 120;
pub(super) const SIGNER_TIMEOUT_MINUTES: i64 = 10;

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
fn action(name: &str, uses: &str, with: &[(&str, &str)]) -> Yaml {
    map(&[
        ("name", Yaml::str(name)),
        ("uses", Yaml::str(uses)),
        (
            "with",
            Yaml::Map(
                with.iter()
                    .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
                    .collect(),
            ),
        ),
    ])
}
fn run(name: &str, command: &str) -> Yaml {
    map(&[
        ("name", Yaml::str(name)),
        ("shell", Yaml::str("bash")),
        ("run", Yaml::str(command)),
    ])
}
fn permissions(items: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn qualifier_steps(pins: &super::ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut steps_list = vec![action(
        "Checkout exact source",
        CHECKOUT_USES,
        &[
            ("ref", "${{ github.sha }}"),
            ("persist-credentials", "false"),
        ],
    )];
    steps_list.extend(generator_release::product_setup_steps(
        pins,
        ReleaseTarget::LinuxX86_64,
    )?);
    steps_list.push(generator_release::command_step(
        "Install pinned Rust and MBX",
        &pins.install_build_tools_argv,
    )?);
    steps_list.push(generator_release::verify_rust_step(pins)?);
    steps_list.push(map(&[
        ("name", Yaml::str("Discover qualifier identity")),
        ("id", Yaml::str("identity")),
        (
            "env",
            map(&[("GH_TOKEN", Yaml::str("${{ github.token }}"))]),
        ),
        (
            "run",
            Yaml::str(format!(
                "{}\nsource scripts/resolve-runner-family-producer.sh qualifier",
                generator_release::gh_function(pins)?
            )),
        ),
    ]));
    steps_list.push(action(
        "Download exact CI image artifact",
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
    ));
    steps_list.push(run("Build qualification tool", "set -eu\nexport CARGO_TARGET_DIR=\"$RUNNER_TEMP/runner-family-qualifier-target\"\ncargo build --manifest-path crates/velnor-runner/Cargo.toml --locked -p velnor-runner-host --bin velnor-resource-probe-release"));
    steps_list.push(map(&[("name", Yaml::str("Qualify exact three-image family")), ("env", map(&[("PRODUCER_RUN_ID", Yaml::str("${{ needs.discover-producer.outputs.producer_run_id }}")), ("PRODUCER_RUN_ATTEMPT", Yaml::str("${{ needs.discover-producer.outputs.producer_run_attempt }}")), ("QUALIFIER_JOB_ID", Yaml::str("${{ steps.identity.outputs.qualifier_job_id }}"))])), ("run", Yaml::str("set -eu\nmkdir -m 0700 \"$RUNNER_TEMP/runner-family-receipt\"\nassets=\"$RUNNER_TEMP/runner-family-candidate\"\n\"$RUNNER_TEMP/runner-family-qualifier-target/debug/velnor-resource-probe-release\" qualify-runner-image-archives \"$GITHUB_WORKSPACE\" \"$GITHUB_SHA\" \"$assets/velnor-runner-linux-amd64.tar\" \"$assets/velnor-dind-linux-amd64.tar\" \"$assets/velnor-resource-probe-linux-amd64.tar\" \"$assets/runner-image-provenance.json\" \"$assets/dind-image-provenance.json\" \"$assets/resource-probe-image-provenance.json\" \"$assets\" \"$assets/runner-image-publication.json\" \"$assets/dind-image-publication.json\" \"$assets/resource-probe-image-publication.json\" \"$PRODUCER_RUN_ID\" \"$PRODUCER_RUN_ATTEMPT\" \"$GITHUB_RUN_ID\" \"$GITHUB_RUN_ATTEMPT\" \"$QUALIFIER_JOB_ID\" /var/run/docker.sock \"$RUNNER_TEMP/runner-family-receipt/RUNNER_IMAGE_QUALIFICATION.json\""))]));
    steps_list.push(map(&[
        ("name", Yaml::str("Upload qualification receipt")),
        ("id", Yaml::str("receipt")),
        ("uses", Yaml::str(steps::UPLOAD_ARTIFACT_USES)),
        (
            "with",
            map(&[
                (
                    "name",
                    Yaml::str("runner-image-qualification-${{ github.run_attempt }}"),
                ),
                (
                    "path",
                    Yaml::str(
                        "${{ runner.temp }}/runner-family-receipt/RUNNER_IMAGE_QUALIFICATION.json",
                    ),
                ),
                ("if-no-files-found", Yaml::str("error")),
            ]),
        ),
    ]));
    Ok(steps_list)
}

fn discovery_job(pins: &super::ProductReleasePins) -> Result<Yaml, RenderError> {
    let discovery_step = map(&[
        ("name", Yaml::str("Discover exact producer")),
        ("id", Yaml::str("producer")),
        (
            "env",
            map(&[("GH_TOKEN", Yaml::str("${{ github.token }}"))]),
        ),
        (
            "run",
            Yaml::str(format!(
                "{}\nsource scripts/resolve-runner-family-producer.sh",
                generator_release::gh_function(pins)?
            )),
        ),
    ]);
    let mut discovery_steps = vec![action(
        "Checkout exact source",
        CHECKOUT_USES,
        &[
            ("ref", "${{ github.sha }}"),
            ("persist-credentials", "false"),
        ],
    )];
    discovery_steps.extend(generator_release::product_setup_steps(
        pins,
        ReleaseTarget::LinuxX86_64,
    )?);
    discovery_steps.push(discovery_step);
    Ok(map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(360)),
        (
            "permissions",
            permissions(&[("actions", "read"), ("contents", "read")]),
        ),
        (
            "outputs",
            map(&[
                (
                    "producer_run_id",
                    Yaml::str("${{ steps.producer.outputs.producer_run_id }}"),
                ),
                (
                    "producer_run_attempt",
                    Yaml::str("${{ steps.producer.outputs.producer_run_attempt }}"),
                ),
                (
                    "producer_artifact_id",
                    Yaml::str("${{ steps.producer.outputs.producer_artifact_id }}"),
                ),
            ]),
        ),
        ("steps", Yaml::Seq(discovery_steps)),
    ]))
}

pub(super) fn caller(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("runner_qualification_pins_missing".into()))?;
    let steps_list = qualifier_steps(pins)?;
    let discovery = discovery_job(pins)?;
    let qualifier = map(&[
        ("needs", seq(&["discover-producer"])),
        ("name", Yaml::str("qualify-images")),
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(QUALIFIER_TIMEOUT_MINUTES)),
        (
            "permissions",
            permissions(&[("actions", "read"), ("contents", "read")]),
        ),
        (
            "outputs",
            map(&[(
                "receipt_artifact_id",
                Yaml::str("${{ steps.receipt.outputs.artifact-id }}"),
            )]),
        ),
        (
            "steps",
            Yaml::Seq(crate::schema2::features::guard_artifact_downloads(
                steps_list,
            )),
        ),
    ]);
    let signer = map(&[
        ("needs", seq(&["qualify-images"])),
        (
            "uses",
            Yaml::uses("./.github/workflows/runner-image-qualification-attest.yml"),
        ),
        (
            "permissions",
            permissions(&[
                ("contents", "read"),
                ("actions", "read"),
                ("attestations", "write"),
                ("id-token", "write"),
            ]),
        ),
        (
            "with",
            map(&[(
                "receipt_artifact_id",
                Yaml::str("${{ needs.qualify-images.outputs.receipt_artifact_id }}"),
            )]),
        ),
    ]);
    Ok(map(&[
        ("name", Yaml::str("Runner image qualification")),
        (
            "concurrency",
            map(&[
                (
                    "group",
                    Yaml::str("runner-image-qualification-caller-${{ github.ref }}"),
                ),
                ("cancel-in-progress", Yaml::Bool(true)),
            ]),
        ),
        ("on", map(&[("push", map(&[("branches", seq(&["main"]))]))])),
        ("permissions", permissions(&[("contents", "read")])),
        (
            "jobs",
            map(&[
                ("discover-producer", discovery),
                ("qualify-images", qualifier),
                ("attest-qualification", signer),
            ]),
        ),
    ]))
}

pub(super) fn signer() -> Yaml {
    let download = action(
        "Download exact qualification receipt",
        steps::DOWNLOAD_ARTIFACT_USES,
        &[
            ("artifact-ids", "${{ inputs.receipt_artifact_id }}"),
            ("path", "${{ runner.temp }}/runner-image-qualification"),
        ],
    );
    let attest = action(
        "Attest qualification receipt",
        steps::ATTEST_BUILD_PROVENANCE_USES,
        &[(
            "subject-path",
            "${{ runner.temp }}/runner-image-qualification/RUNNER_IMAGE_QUALIFICATION.json",
        )],
    );
    let job = map(&[
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(SIGNER_TIMEOUT_MINUTES)),
        (
            "permissions",
            permissions(&[
                ("contents", "read"),
                ("actions", "read"),
                ("attestations", "write"),
                ("id-token", "write"),
            ]),
        ),
        (
            "steps",
            Yaml::Seq(crate::schema2::features::guard_artifact_downloads(vec![
                download, attest,
            ])),
        ),
    ]);
    map(&[
        ("name", Yaml::str("Attest runner image qualification")),
        (
            "on",
            map(&[(
                "workflow_call",
                map(&[(
                    "inputs",
                    map(&[(
                        "receipt_artifact_id",
                        map(&[
                            ("type", Yaml::str("string")),
                            ("required", Yaml::Bool(true)),
                        ]),
                    )]),
                )]),
            )]),
        ),
        ("jobs", map(&[("attest", job)])),
    ])
}
