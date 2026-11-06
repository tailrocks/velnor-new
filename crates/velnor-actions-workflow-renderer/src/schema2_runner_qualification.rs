// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Schema2WorkflowRequest,
    ci_image_fetch::{self, CandidateFamily},
    ci_image_producer::SCRUB_EXEC,
    generator_release,
    job_image_candidate::native,
};
use crate::{
    RenderError,
    render::RenderedFile,
    runs_on::runs_on_yaml,
    steps::{
        ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
    },
    yaml::Yaml,
};
use velnor_actions_contract::ReleaseTarget;

const CALLER: &str = ".github/workflows/runner-image-qualification.yml";
const SIGNER: &str = ".github/workflows/runner-image-qualification-attest.yml";

fn map(fields: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
fn string(s: &str) -> Yaml {
    Yaml::str(s)
}
fn scrubbed(name: &str, script: &str, mut environment: Vec<(&str, Yaml)>) -> Yaml {
    environment.push(("VELNOR_QUALIFY_SCRIPT", string(script)));
    map(vec![
        ("name", string(name)),
        ("shell", string("bash")),
        (
            "run",
            string(&format!(
                "set -- bash -c \"$VELNOR_QUALIFY_SCRIPT\"\n{SCRUB_EXEC}"
            )),
        ),
        ("env", map(environment)),
    ])
}
fn permissions(write: bool) -> Yaml {
    let mut fields = vec![("actions", string("read")), ("contents", string("read"))];
    if write {
        fields.extend([
            ("artifact-metadata", string("write")),
            ("attestations", string("write")),
            ("id-token", string("write")),
        ]);
    }
    map(fields)
}

fn identity_step(pins: &super::ProductReleasePins) -> Result<Yaml, RenderError> {
    let gh = crate::commands::join_argv_for_run(&pins.gh_argv)?;
    let job_lookup = format!(
        r#"set -euo pipefail
: > "$RUNNER_TEMP/qualifier-jobs.jsonl"
page=1
while [ "$page" -le 10 ]; do
  {gh} api "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/attempts/$GITHUB_RUN_ATTEMPT/jobs?per_page=100&page=$page" > "$RUNNER_TEMP/qualifier-page.json"
  count=$(jq -er '.jobs | length | select(. <= 100)' "$RUNNER_TEMP/qualifier-page.json")
  jq -c '.jobs[] | select(.name == "qualify-images")' "$RUNNER_TEMP/qualifier-page.json" >> "$RUNNER_TEMP/qualifier-jobs.jsonl"
  [ "$count" -lt 100 ] && break
  page=$((page + 1))
done
[ "$page" -le 10 ]
id=$(jq -s -er --argjson run "$GITHUB_RUN_ID" --argjson attempt "$GITHUB_RUN_ATTEMPT" 'if length == 1 and .[0].id > 0 and .[0].run_id == $run and .[0].run_attempt == $attempt and .[0].status == "in_progress" then .[0].id else error("qualifier identity") end' "$RUNNER_TEMP/qualifier-jobs.jsonl")
printf 'id=%s\n' "$id" >> "$GITHUB_OUTPUT""#
    );
    let job_identity = map(vec![
        ("name", string("Identify qualifier job")),
        ("id", string("identity")),
        ("shell", string("bash")),
        ("run", string(&job_lookup)),
        (
            "env",
            map(vec![("GH_TOKEN", string("${{ secrets.GITHUB_TOKEN }}"))]),
        ),
    ]);
    Ok(job_identity)
}

fn qualification_step() -> Yaml {
    scrubbed(
        "Qualify exact CI images",
        include_str!("schema2_runner_qualification.sh"),
        vec![
            (
                "PRODUCER_RUN_ID",
                string("${{ steps.locate.outputs.producer_run_id }}"),
            ),
            (
                "PRODUCER_RUN_ATTEMPT",
                string("${{ steps.locate.outputs.producer_run_attempt }}"),
            ),
            (
                "QUALIFIER_JOB_ID",
                string("${{ steps.identity.outputs.id }}"),
            ),
        ],
    )
}

fn qualifier_job(
    request: &Schema2WorkflowRequest,
    pins: &super::ProductReleasePins,
) -> Result<Yaml, RenderError> {
    let job_identity = identity_step(pins)?;
    let qualify = qualification_step();
    let steps = vec![
        map(vec![
            ("name", string("Check out exact source")),
            ("uses", string(CHECKOUT_USES)),
            ("with", map(vec![("persist-credentials", string("false"))])),
        ]),
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned build tools",
            &pins.install_build_tools_argv,
        )?,
        generator_release::mise_install_step(
            "Install pinned resource probe tools",
            &pins.install_resource_probe_release_tools_argv,
        )?,
        generator_release::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
        generator_release::verify_rust_step(pins)?,
        scrubbed(
            "Build exact-source image maintenance",
            &native::build_script(pins)?,
            vec![],
        ),
        scrubbed(
            "Build exact-source runtime qualifier",
            &crate::commands::join_argv_for_run(&pins.resource_probe_release_build_argv)?,
            vec![],
        ),
        ci_image_fetch::step(pins, CandidateFamily::Runner)?,
        job_identity,
        qualify,
        map(vec![
            ("name", string("Upload qualification proof")),
            ("id", string("proof")),
            ("uses", string(UPLOAD_ARTIFACT_USES)),
            (
                "with",
                map(vec![
                    (
                        "name",
                        string("velnor-runner-image-qualification-${{ github.run_attempt }}"),
                    ),
                    (
                        "path",
                        string(
                            "${{ runner.temp }}/runner-candidates/RUNNER_IMAGE_QUALIFICATION.json",
                        ),
                    ),
                    ("if-no-files-found", string("error")),
                    ("retention-days", Yaml::Int(7)),
                ]),
            ),
        ]),
    ];
    let qualifier = map(vec![
        ("name", string("qualify-images")),
        ("runs-on", runs_on_yaml(&request.hosted_label)?),
        ("timeout-minutes", Yaml::Int(90)),
        ("permissions", permissions(false)),
        (
            "outputs",
            map(vec![(
                "artifact_id",
                string("${{ steps.proof.outputs.artifact-id }}"),
            )]),
        ),
        ("steps", Yaml::Seq(steps)),
    ]);
    Ok(qualifier)
}

fn caller_workflow(qualifier: Yaml) -> Yaml {
    let signer_call = map(vec![
        ("needs", Yaml::Seq(vec![string("qualify-images")])),
        (
            "uses",
            string("./.github/workflows/runner-image-qualification-attest.yml"),
        ),
        (
            "with",
            map(vec![(
                "artifact_id",
                string("${{ needs.qualify-images.outputs.artifact_id }}"),
            )]),
        ),
        ("permissions", permissions(true)),
    ]);
    map(vec![
        ("name", string("Runner image qualification")),
        (
            "on",
            map(vec![(
                "push",
                map(vec![("branches", Yaml::Seq(vec![string("main")]))]),
            )]),
        ),
        (
            "concurrency",
            map(vec![
                (
                    "group",
                    string("${{ github.workflow }}-caller-${{ github.ref }}"),
                ),
                ("cancel-in-progress", Yaml::Bool(true)),
            ]),
        ),
        ("permissions", map(vec![("contents", string("read"))])),
        (
            "jobs",
            map(vec![("qualify-images", qualifier), ("attest", signer_call)]),
        ),
    ])
}

fn signer_workflow(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let signer = map(vec![
        ("name", string("Runner image qualification attestation")),
        (
            "on",
            map(vec![(
                "workflow_call",
                map(vec![(
                    "inputs",
                    map(vec![(
                        "artifact_id",
                        map(vec![
                            ("required", Yaml::Bool(true)),
                            ("type", string("string")),
                        ]),
                    )]),
                )]),
            )]),
        ),
        ("permissions", map(vec![("contents", string("read"))])),
        (
            "jobs",
            map(vec![(
                "attest",
                map(vec![
                    ("runs-on", runs_on_yaml(&request.hosted_label)?),
                    ("timeout-minutes", Yaml::Int(30)),
                    ("permissions", permissions(true)),
                    (
                        "steps",
                        Yaml::Seq(vec![
                            map(vec![
                                ("name", string("Download exact proof")),
                                ("uses", string(DOWNLOAD_ARTIFACT_USES)),
                                (
                                    "with",
                                    map(vec![
                                        ("artifact-ids", string("${{ inputs.artifact_id }}")),
                                        ("path", string("${{ runner.temp }}/qualification")),
                                    ]),
                                ),
                            ]),
                            map(vec![
                                ("name", string("Attest qualification proof")),
                                ("uses", string(ATTEST_BUILD_PROVENANCE_USES)),
                                (
                                    "with",
                                    map(vec![(
                                        "subject-path",
                                        string(
                                            "${{ runner.temp }}/qualification/RUNNER_IMAGE_QUALIFICATION.json",
                                        ),
                                    )]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            )]),
        ),
    ]);
    Ok(signer)
}

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Vec<RenderedFile>, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("image_release_pins_missing".to_owned()))?;
    let caller = caller_workflow(qualifier_job(request, pins)?);
    let signer = signer_workflow(request)?;
    Ok(vec![
        super::file(CALLER, &request.version, &caller)?,
        super::file(SIGNER, &request.version, &signer)?,
    ])
}
