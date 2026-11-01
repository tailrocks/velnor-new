// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{JobTimeout, ReleaseTarget, RoutingWorkflow};

use crate::yaml::Yaml;
use crate::{ProductReleasePins, RenderError, Schema2WorkflowRequest};

#[path = "schema2_generator_source_qualification_jobs.rs"]
mod jobs;

use jobs::{
    GATHER_JOB_ID, GATHER_JOB_NAME, GATHER_PHASE, PRELUDE_AFTER_MISE, PRELUDE_BEFORE_MISE,
    PRODUCER_JOBS, Phase, ProducerJob,
};

pub(super) const WORKFLOW_PATH: &str = ".github/workflows/source-qualification.yml";

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request.product_release.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("source_qualification_requires_generator_release".to_owned())
    })?;
    if !request
        .workflows
        .contains(&RoutingWorkflow::GeneratorRelease)
    {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_requires_generator_release".to_owned(),
        ));
    }
    let runner = super::RunnerSpec::hosted(&request.hosted_label)?;
    let mut jobs = Vec::new();
    for producer in PRODUCER_JOBS {
        jobs.push((
            producer.id.to_owned(),
            producer_job(&runner, pins, producer)?,
        ));
    }
    jobs.push((GATHER_JOB_ID.to_owned(), gather_job(&runner)?));
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Source qualification")),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                ("pull_request".to_owned(), Yaml::Map(Vec::new())),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("permissions".to_owned(), read_permissions()),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("source-qualification-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

fn read_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}

fn producer_job(
    runner: &super::RunnerSpec,
    pins: &ProductReleasePins,
    producer: &ProducerJob,
) -> Result<Yaml, RenderError> {
    let mut steps = vec![checkout_step()];
    steps.extend(phase_steps(PRELUDE_BEFORE_MISE));
    steps.push(mise_step(pins)?);
    steps.extend(phase_steps(PRELUDE_AFTER_MISE));
    steps.extend(phase_steps(producer.phases));
    steps.push(upload_step(&producer_artifact(producer.id)));
    let mut fields = job_head(runner, producer.name)?;
    fields.extend([
        ("env".to_owned(), job_env()),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ]);
    runner.push_default_shell(&mut fields, false);
    Ok(Yaml::Map(fields))
}

fn gather_job(runner: &super::RunnerSpec) -> Result<Yaml, RenderError> {
    let mut steps = vec![checkout_step()];
    steps.extend(
        PRODUCER_JOBS
            .iter()
            .map(|producer| download_step(producer.id, &producer_artifact(producer.id))),
    );
    steps.extend(phase_steps(&[GATHER_PHASE]));
    steps.push(upload_step(
        "source-qualification-${{ github.run_id }}-${{ github.run_attempt }}",
    ));
    let mut fields = job_head(runner, GATHER_JOB_NAME)?;
    fields.insert(
        1,
        (
            "needs".to_owned(),
            Yaml::Seq(PRODUCER_JOBS.iter().map(|job| Yaml::str(job.id)).collect()),
        ),
    );
    fields.insert(2, ("if".to_owned(), Yaml::str("always()")));
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    runner.push_default_shell(&mut fields, false);
    Ok(Yaml::Map(fields))
}

fn job_head(runner: &super::RunnerSpec, name: &str) -> Result<Vec<(String, Yaml)>, RenderError> {
    Ok(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runner.runs_on.clone()),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(
                JobTimeout::new(90)
                    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?
                    .minutes(),
            )),
        ),
        ("permissions".to_owned(), read_permissions()),
    ])
}

fn producer_artifact(job_id: &str) -> String {
    format!("source-qualification-{job_id}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}")
}

fn phase_steps(phases: &[Phase]) -> Vec<Yaml> {
    phases
        .iter()
        .map(|(name, phase, always)| phase_step(name, phase, *always))
        .collect()
}

fn job_env() -> Yaml {
    Yaml::Map(vec![
        ("MBX_GC_AUTO".to_owned(), Yaml::str("0")),
        (
            "MISE_BACKENDS_JACTIONLINT".to_owned(),
            Yaml::str("github:jdx/jactionlint"),
        ),
        (
            "MISE_BACKENDS_SHELLCHECK".to_owned(),
            Yaml::str("github:koalaman/shellcheck"),
        ),
        ("MISE_NO_CONFIG".to_owned(), Yaml::str("1")),
        (
            "PR_HEAD_SHA".to_owned(),
            Yaml::str("${{ github.event.pull_request.head.sha || '' }}"),
        ),
        (
            "PR_BASE_SHA".to_owned(),
            Yaml::str("${{ github.event.pull_request.base.sha || '' }}"),
        ),
    ])
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Checkout exact event source")),
        ("uses".to_owned(), Yaml::str(crate::steps::CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
                ("fetch-depth".to_owned(), Yaml::str("2")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
            ]),
        ),
    ])
}

fn mise_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let mut step = super::generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?;
    let Yaml::Map(fields) = &mut step else {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_mise_setup_not_map".to_owned(),
        ));
    };
    fields[0].1 = Yaml::str("Set up pinned Mise");
    Ok(step)
}

fn phase_step(name: &str, phase: &str, always: bool) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if always {
        fields.push(("if".to_owned(), Yaml::str("always()")));
    }
    fields.extend([
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "bash scripts/generator-release/source-qualification.sh {phase}"
            )),
        ),
    ]);
    Yaml::Map(fields)
}

fn upload_step(artifact: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload source qualification evidence"),
        ),
        ("if".to_owned(), Yaml::str("always()")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                (
                    "path".to_owned(),
                    Yaml::str("${{ runner.temp }}/source-qualification-artifacts/"),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("include-hidden-files".to_owned(), Yaml::Bool(true)),
                ("retention-days".to_owned(), Yaml::Int(14)),
            ]),
        ),
    ])
}

fn download_step(job_id: &str, artifact: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Download {job_id} evidence")),
        ),
        ("if".to_owned(), Yaml::str("always()")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                (
                    "path".to_owned(),
                    Yaml::str(format!(
                        "${{{{ runner.temp }}}}/source-qualification-jobs/{job_id}"
                    )),
                ),
            ]),
        ),
    ])
}
