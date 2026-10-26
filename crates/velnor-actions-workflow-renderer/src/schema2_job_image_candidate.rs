// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{JobTimeout, RoutingWorkflow};

use crate::steps::{
    ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
};
use crate::{RenderError, render::RenderedFile, runs_on::runs_on_yaml, yaml::Yaml};

use super::Schema2WorkflowRequest;

pub(super) const WORKFLOW_PATH: &str = ".github/workflows/job-image-candidate.yml";

const ARCHIVE: &str = "job-image.oci.tar";
const RECORD: &str = "job-image-candidate.json";
const ARTIFACT_NAME: &str = "velnor-job-image-candidate-${{ github.run_attempt }}";
const PUBLICATION_EVIDENCE: &str =
    "velnor-job-image-publication-evidence-${{ github.run_attempt }}";
const ATTESTATION_CANDIDATE: &str = "${{ runner.temp }}/privatecandidate";
const ATTESTATION_RUNTIME_CANDIDATE: &str = "$RUNNER_TEMP/privatecandidate";
#[path = "schema2_job_image_native.rs"]
mod native;

pub(super) fn render_candidate_workflow(
    request: &Schema2WorkflowRequest,
) -> Result<RenderedFile, RenderError> {
    if !request.workflows.contains(&RoutingWorkflow::ImageRelease) {
        return Err(RenderError::InvalidWorkflow(
            "image_release_not_requested".to_owned(),
        ));
    }
    let workflow = render(request)?;
    super::file(WORKFLOW_PATH, &request.version, &workflow)
}

fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let artifact_id = "${{ needs.build-image-candidate.outputs.artifact_id }}";
    let pins = request.product_release.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("image_candidate_missing_native_pins".to_owned())
    })?;
    let build = build_job(hosted.clone(), pins)?;
    let attest = attest_job(hosted.clone(), artifact_id, pins)?;
    let publish = publish_job(hosted, artifact_id);
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Job image candidate")),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                ("push".to_owned(), Yaml::Map(Vec::new())),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("concurrency".to_owned(), workflow_concurrency()),
        (
            "permissions".to_owned(),
            permissions(&[("contents", "read")]),
        ),
        ("jobs".to_owned(), Yaml::Map(vec![build, attest, publish])),
    ]))
}

fn workflow_concurrency() -> Yaml {
    Yaml::Map(vec![
        (
            "group".to_owned(),
            Yaml::str("velnor-${{ github.workflow }}-${{ github.event_name }}-${{ github.ref }}"),
        ),
        (
            "cancel-in-progress".to_owned(),
            Yaml::str("${{ github.event_name == 'push' }}"),
        ),
    ])
}

fn build_job(
    hosted: Yaml,
    pins: &super::ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let binary = native::BINARY;
    let closure = native::CLOSURE;
    let build_command = format!(
        "\"{binary}\" image-maintenance build-job-candidate --root \"$GITHUB_WORKSPACE\" --source-commit \"$GITHUB_SHA\" --runner-temp \"$RUNNER_TEMP\" --archive-path \"$RUNNER_TEMP/{ARCHIVE}\" --record-path \"$RUNNER_TEMP/{RECORD}\" --source-closure-path \"$RUNNER_TEMP/{closure}\""
    );
    let mut steps = vec![checkout_step("Use the source commit selected by GitHub.")];
    steps.extend(native::source_steps(pins)?);
    steps.extend([
        run_step("Verify native implementation source closure", &format!("\"{binary}\" image-maintenance check-job-source --root \"$GITHUB_WORKSPACE\" --source-commit \"$GITHUB_SHA\" --source-closure-path \"$RUNNER_TEMP/{closure}\"")),
        run_step("Install the Buildx plugin from .velnor/runner.lock", &format!("\"{binary}\" image-maintenance install-buildx --root \"$GITHUB_WORKSPACE\" --runner-temp \"$RUNNER_TEMP\"")),
        run_step("Build the job image candidate", &build_command),
        upload_step(),
    ]);
    let fields = vec![
        (
            "name".to_owned(),
            Yaml::str("Build Linux job image candidate"),
        ),
        ("runs-on".to_owned(), hosted),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(JobTimeout::PRODUCT_COMPILER.minutes())),
        ),
        (
            "permissions".to_owned(),
            permissions(&[("contents", "read")]),
        ),
        (
            "outputs".to_owned(),
            Yaml::Map(vec![(
                "artifact_id".to_owned(),
                Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
            )]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ];
    Ok(("build-image-candidate".to_owned(), Yaml::Map(fields)))
}

fn attest_job(
    hosted: Yaml,
    artifact_id: &str,
    pins: &super::ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let archive = format!("{ATTESTATION_RUNTIME_CANDIDATE}/{ARCHIVE}");
    let record = format!("{ATTESTATION_RUNTIME_CANDIDATE}/{RECORD}");
    let binary = native::BINARY;
    let closure = native::CLOSURE;
    let validate = format!(
        "\"{binary}\" image-maintenance validate-job-candidate --root \"$GITHUB_WORKSPACE\" --archive-path \"{archive}\" --record-path \"{record}\" --source-closure-path \"{ATTESTATION_RUNTIME_CANDIDATE}/{closure}\""
    );
    let attest = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest all candidate bytes.")),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                "subject-path".to_owned(),
                Yaml::str(format!(
                    "{ATTESTATION_CANDIDATE}/{ARCHIVE}\n{ATTESTATION_CANDIDATE}/{RECORD}\n{ATTESTATION_CANDIDATE}/{closure}"
                )),
            )]),
        ),
    ]);
    let mut steps = vec![checkout_step("Use the source commit selected by GitHub.")];
    steps.extend(native::source_steps(pins)?);
    steps.extend([
        artifact_id_check_step(artifact_id),
        download_step(artifact_id, ATTESTATION_CANDIDATE),
        run_step("Do a check of source and archive identity", &validate),
        attest,
    ]);
    let fields = vec![
        (
            "name".to_owned(),
            Yaml::str("Attest Linux job image candidate"),
        ),
        ("runs-on".to_owned(), hosted),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(JobTimeout::PRODUCT_COMPILER.minutes())),
        ),
        (
            "needs".to_owned(),
            Yaml::Seq(vec![Yaml::str("build-image-candidate")]),
        ),
        (
            "permissions".to_owned(),
            permissions(&[
                ("actions", "read"),
                ("artifact-metadata", "write"),
                ("attestations", "write"),
                ("contents", "read"),
                ("id-token", "write"),
            ]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ];
    Ok(("attest-image-candidate".to_owned(), Yaml::Map(fields)))
}

fn publish_job(hosted: Yaml, artifact_id: &str) -> (String, Yaml) {
    let archive = format!("candidate/{ARCHIVE}");
    let record = format!("candidate/{RECORD}");
    let result = "$RUNNER_TEMP/job-image-publication.json";
    let publish = format!(
        "python3 images/job/ubuntu-26.04/candidate/publish.py --root . --archive \"{archive}\" --record \"{record}\" --result \"{result}\""
    );
    let mut publish_step = run_step(
        "Publish the job image with its signed build evidence.",
        &publish,
    );
    if let Yaml::Map(fields) = &mut publish_step {
        fields.push((
            "env".to_owned(),
            Yaml::Map(vec![
                ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
                (
                    "VELNOR_CANDIDATE_ARTIFACT_ID".to_owned(),
                    Yaml::str(artifact_id),
                ),
            ]),
        ));
    }
    let steps = vec![
        checkout_step("Use the source commit selected by GitHub."),
        artifact_id_check_step(artifact_id),
        download_step(artifact_id, "candidate"),
        publish_step,
        publication_evidence_upload_step("${{ runner.temp }}/job-image-publication.json"),
    ];
    let fields = vec![
        ("name".to_owned(), Yaml::str("Publish Linux job image")),
        ("runs-on".to_owned(), hosted),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(JobTimeout::PUBLISH.minutes())),
        ),
        (
            "if".to_owned(),
            Yaml::str(
                "${{ github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' }}",
            ),
        ),
        (
            "needs".to_owned(),
            Yaml::Seq(vec![
                Yaml::str("build-image-candidate"),
                Yaml::str("attest-image-candidate"),
            ]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("${{ github.repository }}-job-image-publish"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "permissions".to_owned(),
            permissions(&[
                ("actions", "read"),
                ("attestations", "read"),
                ("contents", "read"),
                ("packages", "write"),
            ]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ];
    ("publish-job-image".to_owned(), Yaml::Map(fields))
}

fn checkout_step(name: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
                ("fetch-depth".to_owned(), Yaml::Int(1)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

fn run_step(name: &str, command: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(command)),
    ])
}

fn upload_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload candidate archive and record"),
        ),
        ("id".to_owned(), Yaml::str("upload")),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(ARTIFACT_NAME)),
                (
                    "path".to_owned(),
                    Yaml::str(format!(
                        "${{{{ runner.temp }}}}/{ARCHIVE}\n${{{{ runner.temp }}}}/{RECORD}\n${{{{ runner.temp }}}}/{}",
                        native::CLOSURE
                    )),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
            ]),
        ),
    ])
}

fn publication_evidence_upload_step(result: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload job image publication evidence"),
        ),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(PUBLICATION_EVIDENCE)),
                ("path".to_owned(), Yaml::str(result)),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(7)),
            ]),
        ),
    ])
}

fn artifact_id_check_step(artifact_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Examine the uploaded artifact ID"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![("ARTIFACT_ID".to_owned(), Yaml::str(artifact_id))]),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "if [[ ! \"$ARTIFACT_ID\" =~ ^[1-9][0-9]*$ ]]; then\n  printf '%s\\n' 'The artifact ID is missing or incorrect.' >&2\n  exit 1\nfi",
            ),
        ),
    ])
}

fn download_step(artifact_id: &str, destination: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Download the job image candidate from this workflow run"),
        ),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("path".to_owned(), Yaml::str(destination)),
            ]),
        ),
    ])
}

fn permissions(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
