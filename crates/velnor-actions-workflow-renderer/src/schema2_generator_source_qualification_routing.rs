// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::SourceQualificationPlan;
use crate::{RenderError, yaml::Yaml};

pub(super) fn plan_job(plan: &SourceQualificationPlan) -> Result<Yaml, RenderError> {
    let mut job = plan.job.clone();
    let Yaml::Map(fields) = &mut job else {
        return Err(invalid());
    };
    fields.push((
        "if".to_owned(),
        Yaml::str("github.event_name == 'pull_request'"),
    ));
    let Some((_, Yaml::Map(outputs))) = fields.iter_mut().find(|(key, _)| key == "outputs") else {
        return Err(invalid());
    };
    outputs.clear();
    outputs.push((
        "source_plan_sha256".to_owned(),
        Yaml::str("${{ steps.source_plan_proof.outputs.plan_sha256 }}"),
    ));
    outputs.push((
        "source_matrix_sha256".to_owned(),
        Yaml::str("${{ steps.source_plan_proof.outputs.matrix_sha256 }}"),
    ));
    outputs.push((
        "source_binary_sha256".to_owned(),
        Yaml::str("${{ steps.source_plan_proof.outputs.binary_sha256 }}"),
    ));
    outputs.push((
        "plan_attempt".to_owned(),
        Yaml::str("${{ github.run_attempt }}"),
    ));
    let Some((_, Yaml::Seq(steps))) = fields.iter_mut().find(|(key, _)| key == "steps") else {
        return Err(invalid());
    };
    let upload = steps.iter().position(|step| matches!(step, Yaml::Map(fields) if fields.iter().any(|(key, value)| key == "name" && *value == Yaml::str("Publish plan")))).ok_or_else(invalid)?;
    let script = format!(
        "set -eu; p=\"$RUNNER_TEMP/velnor/r${{GITHUB_RUN_ID}}-a${{GITHUB_RUN_ATTEMPT}}/plan.json\"; m=\"$RUNNER_TEMP/velnor/r${{GITHUB_RUN_ID}}-a${{GITHUB_RUN_ATTEMPT}}/matrix.json\"; b=\"{}\"; [ -f \"$m\" ] && [ ! -L \"$m\" ] && [ -f \"$p\" ] && [ ! -L \"$p\" ] && [ -f \"$b\" ] && [ ! -L \"$b\" ]; psha=$(sha256sum \"$p\"); msha=$(sha256sum \"$m\"); bsha=$(sha256sum \"$b\"); psha=${{psha%% *}}; msha=${{msha%% *}}; bsha=${{bsha%% *}}; printf 'plan_sha256=%s\\nmatrix_sha256=%s\\nbinary_sha256=%s\\n' \"$psha\" \"$msha\" \"$bsha\" >> \"$GITHUB_OUTPUT\"",
        plan.controller_path
    );
    steps.insert(
        upload,
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Bind qualified plan artifact")),
            ("id".to_owned(), Yaml::str("source_plan_proof")),
            ("run".to_owned(), Yaml::str(script)),
        ]),
    );
    Ok(job)
}

pub(super) fn selection_job(
    plan: &SourceQualificationPlan,
    runner: &super::super::RunnerSpec,
) -> Result<Yaml, RenderError> {
    let routes = serde_json::to_string(&plan.cli_routes).map_err(|_| invalid())?;
    let mut steps =
        crate::non_input_checkout::restrict_checkout_yaml(super::checkout_step(), &plan.non_inputs);
    for step in &mut steps {
        if let Yaml::Map(fields) = step {
            fields.push(("if".to_owned(), Yaml::str("always()")));
        }
    }
    steps.push(download_plan_step());
    steps.push(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Admit source qualification selection"),
        ),
        ("id".to_owned(), Yaml::str("selection")),
        ("if".to_owned(), Yaml::str("always()")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "PLAN_RESULT".to_owned(),
                    Yaml::str("${{ needs.plan.result }}"),
                ),
                (
                    "PLAN_SHA256".to_owned(),
                    Yaml::str("${{ needs.plan.outputs.source_plan_sha256 }}"),
                ),
                (
                    "MATRIX_SHA256".to_owned(),
                    Yaml::str("${{ needs.plan.outputs.source_matrix_sha256 }}"),
                ),
                (
                    "BINARY_SHA256".to_owned(),
                    Yaml::str("${{ needs.plan.outputs.source_binary_sha256 }}"),
                ),
                (
                    "PLAN_ATTEMPT".to_owned(),
                    Yaml::str("${{ needs.plan.outputs.plan_attempt }}"),
                ),
                (
                    "CONTROLLER_VERSION".to_owned(),
                    Yaml::str(&plan.controller_version),
                ),
                (
                    "CONTROLLER_TARGET".to_owned(),
                    Yaml::str("x86_64-unknown-linux-gnu"),
                ),
                ("CLI_ROUTES".to_owned(), Yaml::str(routes)),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str("python3 scripts/generator-release/source-qualification-routing.py select"),
        ),
    ]));
    let gate = vec![
        ("needs".to_owned(), Yaml::Seq(vec![Yaml::str("plan")])),
        ("if".to_owned(), Yaml::str("always()")),
    ];
    let mut fields = super::job_head(runner, "Select source qualification", gate)?;
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "decision".to_owned(),
            Yaml::str("${{ steps.selection.outputs.decision }}"),
        )]),
    ));
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    Ok(Yaml::Map(fields))
}

fn download_plan_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download qualified plan")),
        (
            "if".to_owned(),
            Yaml::str(
                "always() && github.event_name == 'pull_request' && needs.plan.result == 'success'",
            ),
        ),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str(
                        "velnor-plan-r${{ github.run_id }}-a${{ needs.plan.outputs.plan_attempt }}",
                    ),
                ),
                (
                    "path".to_owned(),
                    Yaml::str("${{ runner.temp }}/source-qualification-plan"),
                ),
            ]),
        ),
    ])
}

pub(super) fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("source_qualification_cli_route".to_owned())
}

pub(super) fn gather_job(
    runner: &super::super::RunnerSpec,
    plan: &SourceQualificationPlan,
) -> Result<Yaml, RenderError> {
    let mut steps =
        crate::non_input_checkout::restrict_checkout_yaml(super::checkout_step(), &plan.non_inputs);
    for step in &mut steps {
        if let Yaml::Map(fields) = step {
            fields.push(("if".to_owned(), Yaml::str("always()")));
        }
    }
    let checkout_count = steps.len();
    steps.extend(
        super::PRODUCER_JOBS
            .iter()
            .map(|producer| download_step(producer.id, &super::producer_artifact(producer.id))),
    );
    steps.extend(super::phase_steps(&[super::GATHER_PHASE]));
    steps.push(super::upload_step(
        "source-qualification-${{ github.run_id }}-${{ github.run_attempt }}",
    ));
    for step in steps.iter_mut().skip(checkout_count) {
        gate_gather_step(step);
    }
    steps.push(producers_succeeded_step());
    let gate = vec![
        (
            "needs".to_owned(),
            Yaml::Seq(
                std::iter::once(Yaml::str("plan"))
                    .chain(std::iter::once(Yaml::str("selection")))
                    .chain(super::PRODUCER_JOBS.iter().map(|job| Yaml::str(job.id)))
                    .collect(),
            ),
        ),
        ("if".to_owned(), Yaml::str("always()")),
    ];
    let mut fields = super::job_head(runner, super::GATHER_JOB_NAME, gate)?;
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    runner.push_default_shell(&mut fields, false);
    Ok(Yaml::Map(fields))
}

fn producers_succeeded_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Require planned producer results"),
        ),
        ("if".to_owned(), Yaml::str("always()")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "PLAN_RESULT".to_owned(),
                    Yaml::str("${{ needs.plan.result }}"),
                ),
                (
                    "SELECTION_RESULT".to_owned(),
                    Yaml::str("${{ needs.selection.result }}"),
                ),
                (
                    "DECISION".to_owned(),
                    Yaml::str("${{ needs.selection.outputs.decision }}"),
                ),
                (
                    "ORDINARY_RESULT".to_owned(),
                    Yaml::str("${{ needs.ordinary-harness.result }}"),
                ),
                (
                    "KEEP_RESULT".to_owned(),
                    Yaml::str("${{ needs.keep-harness.result }}"),
                ),
                (
                    "GENERATION_RESULT".to_owned(),
                    Yaml::str("${{ needs.generation-checks.result }}"),
                ),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str("python3 scripts/generator-release/source-qualification-routing.py gather"),
        ),
    ])
}

fn gate_gather_step(step: &mut Yaml) {
    if let Yaml::Map(fields) = step {
        let condition = "always() && needs.selection.outputs.decision == 'run'";
        if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "if") {
            *value = Yaml::str(condition);
        } else {
            fields.push(("if".to_owned(), Yaml::str(condition)));
        }
    }
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
