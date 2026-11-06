// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Schema2WorkflowRequest,
    ci_image_fetch::{self, CandidateFamily},
    generator_release,
};
use crate::{
    RenderError, render::RenderedFile, runs_on::runs_on_yaml, steps::ATTEST_BUILD_PROVENANCE_USES,
    yaml::Yaml,
};
use velnor_actions_contract::{ReleaseTarget, RoutingWorkflow};

pub(super) const WORKFLOW_PATH: &str = ".github/workflows/job-image-candidate.yml";
#[path = "job_image_authority.rs"]
mod authority;
#[path = "schema2_job_image_native.rs"]
pub(super) mod native;

pub(super) fn render_candidate_workflow(
    request: &Schema2WorkflowRequest,
) -> Result<RenderedFile, RenderError> {
    if !request.workflows.contains(&RoutingWorkflow::ImageRelease) {
        return Err(RenderError::InvalidWorkflow(
            "image_release_not_requested".to_owned(),
        ));
    }
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("image_release_pins_missing".to_owned()))?;
    let root = "${{ runner.temp }}/privatecandidate";
    let runtime = "$RUNNER_TEMP/privatecandidate";
    let steps = vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
        ci_image_fetch::step(pins, CandidateFamily::Job)?,
        authority::materialize_step(),
        authority::verify_candidate_step(
            &format!("{runtime}/job-image.oci.tar"),
            &format!("{runtime}/job-image-candidate.json"),
            &format!("{runtime}/job-image-native-source-closure.json"),
        ),
        Yaml::Map(vec![
            (
                "name".to_owned(),
                Yaml::str("Attest exact CI candidate bytes"),
            ),
            ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
            (
                "with".to_owned(),
                Yaml::Map(vec![(
                    "subject-path".to_owned(),
                    Yaml::str(format!(
                        "{root}/job-image.oci.tar\n{root}/job-image-candidate.json\n{root}/job-image-native-source-closure.json"
                    )),
                )]),
            ),
        ]),
    ];
    let job = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest Linux job image candidate"),
        ),
        ("runs-on".to_owned(), runs_on_yaml(&request.hosted_label)?),
        ("timeout-minutes".to_owned(), Yaml::Int(90)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("read")),
                ("contents".to_owned(), Yaml::str("read")),
                ("artifact-metadata".to_owned(), Yaml::str("write")),
                ("attestations".to_owned(), Yaml::str("write")),
                ("id-token".to_owned(), Yaml::str("write")),
            ]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ]);
    let workflow = candidate_document(job);
    super::file(WORKFLOW_PATH, &request.version, &workflow)
}

fn candidate_document(job: Yaml) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Job image candidate")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "push".to_owned(),
                Yaml::Map(vec![(
                    "branches".to_owned(),
                    Yaml::Seq(vec![Yaml::str("main")]),
                )]),
            )]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("${{ github.workflow }}-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(true)),
            ]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![("attest-image-candidate".to_owned(), job)]),
        ),
    ])
}
