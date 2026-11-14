// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{
    CargoCommand, FilterRef, HelperBinary, HelperCommand, HelperRelease, Job, Step, StepKind, When,
};

use crate::rules::{FETCH_HELPER_RUN, RenderedJob, RenderedStep, Value};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE: &str = "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c";
const MISE_VERSION: &str = "2026.11.1";
const NOT_CANCELLED: &str = "${{ !cancelled() }}";
const A_NEED_FAILED: &str =
    "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}";

pub(crate) fn rendered_job(job: &Job) -> RenderedJob {
    RenderedJob {
        id: job.id.to_string(),
        name: job.name.clone(),
        comment: job.comment.clone(),
        needs: job.needs.iter().map(ToString::to_string).collect(),
        condition: condition(job.when),
        runs_on: job.runs_on.as_str().to_owned(),
        timeout_minutes: job.timeout_minutes,
        steps: job.steps.iter().map(rendered_step).collect(),
    }
}

fn condition(when: When) -> Option<String> {
    match when {
        When::Success => None,
        When::NotCancelled => Some(NOT_CANCELLED.to_owned()),
    }
}

fn rendered_step(step: &Step) -> RenderedStep {
    let mut rendered = step_body(&step.kind);
    if let Some(condition) = condition(step.when) {
        rendered.condition = Some(condition);
    }
    rendered.working_directory = step.directory.as_ref().map(ToString::to_string);
    rendered
}

fn step_body(kind: &StepKind) -> RenderedStep {
    match kind {
        StepKind::Checkout { depth } => RenderedStep {
            name: "Check out".to_owned(),
            uses: Some(CHECKOUT.to_owned()),
            with: vec![
                ("fetch-depth".to_owned(), Value::Int(u64::from(*depth))),
                ("persist-credentials".to_owned(), Value::Bool(false)),
            ],
            ..RenderedStep::default()
        },
        StepKind::InstallTools => RenderedStep {
            name: "Install tools".to_owned(),
            uses: Some(MISE.to_owned()),
            with: vec![
                ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
                ("cache".to_owned(), Value::Bool(false)),
            ],
            ..RenderedStep::default()
        },
        StepKind::RustComponents(components) => RenderedStep {
            name: "Add Rust components".to_owned(),
            run: Some(format!("rustup component add {}", components.join(" "))),
            ..RenderedStep::default()
        },
        StepKind::RustTargets(targets) => RenderedStep {
            name: "Add Rust targets".to_owned(),
            run: Some(format!("rustup target add {}", targets.join(" "))),
            ..RenderedStep::default()
        },
        StepKind::Cargo(command) => cargo_step(command),
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
        StepKind::FailWhenANeedFailed => RenderedStep {
            name: "Fail when a Rust job failed or was cancelled".to_owned(),
            condition: Some(A_NEED_FAILED.to_owned()),
            run: Some("exit 1".to_owned()),
            ..RenderedStep::default()
        },
    }
}

fn cargo_step(command: &CargoCommand) -> RenderedStep {
    let (name, run, env): (String, String, Vec<(String, Value)>) = match command {
        CargoCommand::FmtCheck => ("Format".into(), "cargo-fmt --all --check".into(), vec![]),
        CargoCommand::Clippy { extra } => (
            "Lint".into(),
            ["cargo-clippy clippy --workspace --all-targets --locked -- -D warnings"]
                .into_iter()
                .chain(extra.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            vec![],
        ),
        CargoCommand::NextestRun { filter } => (
            "Test".into(),
            format!(
                "cargo-nextest nextest run --workspace --locked --no-fail-fast --ignore-default-filter -E '{}'",
                match filter {
                    FilterRef::All => "all()",
                }
            ),
            vec![],
        ),
        CargoCommand::NextestArchive { file } => (
            "Build the tests".into(),
            format!(
                "cargo-nextest nextest archive --workspace --locked --archive-file '{}'",
                file.as_str().replace('\'', "'\\''")
            ),
            vec![],
        ),
        CargoCommand::Doctest => (
            "Doctests".into(),
            "cargo test --doc --workspace --locked".into(),
            vec![],
        ),
        CargoCommand::Doc => (
            "Documentation".into(),
            "cargo doc --workspace --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'"
                .into(),
            vec![],
        ),
        CargoCommand::CheckAlone { package } => (
            format!("Check {package} alone"),
            format!("cargo check -p {package} --locked"),
            vec![],
        ),
        CargoCommand::Deny => (
            "Dependency policy".into(),
            "cargo-deny check".into(),
            vec![],
        ),
        CargoCommand::Machete => ("Unused dependencies".into(), "cargo-machete".into(), vec![]),
    };
    RenderedStep {
        name,
        env,
        run: Some(run),
        ..RenderedStep::default()
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
