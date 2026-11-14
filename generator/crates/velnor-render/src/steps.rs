// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{CargoCommand, HelperBinary, HelperCommand, HelperRelease, Job, Step, StepKind};

use crate::rules::{FETCH_HELPER_RUN, RenderedJob, RenderedStep, Value};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

pub(crate) fn rendered_job(job: &Job) -> RenderedJob {
    RenderedJob {
        id: job.id.to_string(),
        runs_on: job.runs_on.as_str().to_owned(),
        timeout_minutes: job.timeout_minutes,
        steps: job.steps.iter().map(rendered_step).collect(),
    }
}

fn rendered_step(step: &Step) -> RenderedStep {
    match &step.kind {
        StepKind::Checkout { depth } => RenderedStep {
            name: "Check out".to_owned(),
            uses: Some(CHECKOUT.to_owned()),
            with: vec![
                ("fetch-depth".to_owned(), Value::Int(u64::from(*depth))),
                ("persist-credentials".to_owned(), Value::Bool(false)),
            ],
            ..RenderedStep::default()
        },
        StepKind::Cargo(CargoCommand::FmtCheck { manifest }) => RenderedStep {
            name: format!("Format {manifest}"),
            working_directory: manifest.parent().map(|directory| directory.to_string()),
            run: Some("cargo-fmt --all --check".to_owned()),
            ..RenderedStep::default()
        },
        StepKind::Helper(command) => {
            let arguments = helper_arguments(*command);
            RenderedStep {
                name: format!("velnor-ci {arguments}"),
                run: Some(format!("velnor-ci {arguments}")),
                ..RenderedStep::default()
            }
        }
        StepKind::GeneratorCheck => RenderedStep {
            name: "Check the generated workflow".to_owned(),
            run: Some("velnor-gen check".to_owned()),
            ..RenderedStep::default()
        },
        StepKind::FetchHelper(release) => RenderedStep {
            name: format!("Fetch velnor-ci {}", release.version()),
            env: vec![
                (
                    "SHA".to_owned(),
                    by_platform(release, |binary| &binary.sha256),
                ),
                ("URL".to_owned(), by_platform(release, |binary| &binary.url)),
            ],
            run: Some(FETCH_HELPER_RUN.to_owned()),
            ..RenderedStep::default()
        },
    }
}

fn helper_arguments(command: HelperCommand) -> &'static str {
    match command {
        HelperCommand::InputsSave => "inputs save",
        HelperCommand::InputsRestore => "inputs restore",
    }
}

fn by_platform<T: fmt::Display>(
    release: &HelperRelease,
    field: impl Fn(&HelperBinary) -> &T,
) -> Value {
    let table = release
        .binaries()
        .iter()
        .map(|(platform, binary)| format!("\"{}\":\"{}\"", platform.runner_key(), field(binary)))
        .collect::<Vec<_>>()
        .join(",");
    Value::Selected(format!(
        "${{{{ fromJSON('{{{table}}}')[format('{{0}}-{{1}}', runner.os, runner.arch)] }}}}"
    ))
}
