//! Render the workflow that builds, attests, and publishes a Linux job image.

use velnor_actions_contract::{JobTimeout, RoutingWorkflow};

use crate::steps::{
    ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
};
use crate::{RenderError, render::RenderedFile, runs_on::runs_on_yaml, yaml::Yaml};

use super::Schema2WorkflowRequest;

/// The file path for the job image candidate workflow.
pub(super) const WORKFLOW_PATH: &str = ".github/workflows/job-image-candidate.yml";

const ARCHIVE: &str = "job-image.oci.tar";
const RECORD: &str = "job-image-candidate.json";
const ARTIFACT_NAME: &str = "velnor-job-image-candidate-${{ github.run_attempt }}";
const PUBLICATION_EVIDENCE: &str =
    "velnor-job-image-publication-evidence-${{ github.run_attempt }}";
const BUILDX_INSTALL: &str = r#"python3 - "$GITHUB_WORKSPACE/.velnor/runner.lock" "$RUNNER_TEMP/velnor-docker-config/cli-plugins/docker-buildx" <<'PY'
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

lock = tomllib.loads(Path(sys.argv[1]).read_text())
version = lock["buildx_version"]
asset_url = lock["buildx_asset_url"]
expected_url = f"https://github.com/docker/buildx/releases/download/v{version}/buildx-v{version}.linux-amd64"
expected_sha256 = lock["buildx_sha256"]
if asset_url != expected_url or not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
    raise SystemExit("The Buildx values in .velnor/runner.lock are incorrect.")

plugin = Path(sys.argv[2])
plugin.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
plugin.parent.chmod(0o700)
fd, temporary = tempfile.mkstemp(prefix=".docker-buildx.", dir=plugin.parent)
os.close(fd)
try:
    subprocess.run(
        ["curl", "--retry", "5", "--retry-delay", "2", "--retry-all-errors",
         "--fail", "--silent", "--show-error", "--location", "--output", temporary,
         asset_url],
        check=True,
    )
    with Path(temporary).open("rb") as stream:
        actual_sha256 = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual_sha256 != expected_sha256:
        raise SystemExit("The Buildx checksum is different from the value in .velnor/runner.lock.")
    os.chmod(temporary, 0o755)
    os.replace(temporary, plugin)
    plugin.chmod(0o755)
finally:
    if os.path.exists(temporary):
        os.unlink(temporary)
PY"#;

/// Render the job image candidate workflow on the hosted runner.
///
/// # Errors
///
/// The function returns an error if the request or workflow bytes are incorrect.
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
    let build = build_job(hosted.clone());
    let attest = attest_job(hosted.clone(), artifact_id);
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
        (
            "permissions".to_owned(),
            permissions(&[("contents", "read")]),
        ),
        ("jobs".to_owned(), Yaml::Map(vec![build, attest, publish])),
    ]))
}

fn build_job(hosted: Yaml) -> (String, Yaml) {
    let build_command = format!(
        "DOCKER_CONFIG=\"$RUNNER_TEMP/velnor-docker-config\" python3 images/job/ubuntu-26.04/candidate/candidate.py build --source-commit \"$GITHUB_SHA\" --archive \"$RUNNER_TEMP/{ARCHIVE}\" --record \"$RUNNER_TEMP/{RECORD}\""
    );
    let steps = vec![
        checkout_step("Use the exact source commit"),
        run_step(
            "Install the Buildx plugin from .velnor/runner.lock",
            BUILDX_INSTALL,
        ),
        run_step("Build the job image candidate", &build_command),
        upload_step(),
    ];
    let fields = vec![
        (
            "name".to_owned(),
            Yaml::str("Build Linux job image candidate"),
        ),
        ("runs-on".to_owned(), hosted),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(JobTimeout::CANDIDATE.minutes())),
        ),
        (
            "permissions".to_owned(),
            permissions(&[("contents", "read")]),
        ),
        (
            "outputs".to_owned(),
            Yaml::Map(vec![
                ("artifact_name".to_owned(), Yaml::str(ARTIFACT_NAME)),
                (
                    "artifact_id".to_owned(),
                    Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
                ),
            ]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ];
    ("build-image-candidate".to_owned(), Yaml::Map(fields))
}

fn attest_job(hosted: Yaml, artifact_id: &str) -> (String, Yaml) {
    let archive = format!("candidate/{ARCHIVE}");
    let record = format!("candidate/{RECORD}");
    let validate = format!(
        "python3 images/job/ubuntu-26.04/candidate/candidate.py validate --archive \"{archive}\" --record \"{record}\""
    );
    let attest = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest the exact candidate bytes"),
        ),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                "subject-path".to_owned(),
                Yaml::str(format!("{archive}\n{record}")),
            )]),
        ),
    ]);
    let steps = vec![
        checkout_step("Use the exact source commit"),
        artifact_id_check_step(artifact_id),
        download_step(artifact_id),
        run_step("Do a check of source and archive identity", &validate),
        attest,
    ];
    let fields = vec![
        (
            "name".to_owned(),
            Yaml::str("Attest Linux job image candidate"),
        ),
        ("runs-on".to_owned(), hosted),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(JobTimeout::CANDIDATE.minutes())),
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
    ("attest-image-candidate".to_owned(), Yaml::Map(fields))
}

fn publish_job(hosted: Yaml, artifact_id: &str) -> (String, Yaml) {
    let archive = format!("candidate/{ARCHIVE}");
    let record = format!("candidate/{RECORD}");
    let result = "$RUNNER_TEMP/job-image-publication.json";
    let publish = format!(
        "python3 images/job/ubuntu-26.04/candidate/publish.py --root . --archive \"{archive}\" --record \"{record}\" --result \"{result}\""
    );
    let mut publish_step = run_step("Publish the verified job image", &publish);
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
        checkout_step("Use the exact source commit"),
        artifact_id_check_step(artifact_id),
        download_step(artifact_id),
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
            Yaml::str("Store candidate archive and record"),
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
                        "${{{{ runner.temp }}}}/{ARCHIVE}\n${{{{ runner.temp }}}}/{RECORD}"
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
            Yaml::str("Store job image publication evidence"),
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

fn download_step(artifact_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Get the job image candidate from this workflow run"),
        ),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("path".to_owned(), Yaml::str("candidate")),
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

#[cfg(test)]
#[path = "schema2_job_image_candidate_tests.rs"]
mod tests;
