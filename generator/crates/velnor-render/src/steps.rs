// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, FilterRef, HelperCommand, HelperRelease, Job, RepoPath, Step, StepKind,
    TESTS_RUN_OUTPUT,
};

use crate::gate::{PLAN_STEP, condition, output};
use crate::rules::{RenderedJob, RenderedStep, SETUP_VELNOR_ACTION, Value};

const PREPARE_STEP: &str = "prepare";
const RUN: &str = "velnor-ci run --";

const UPLOAD_ARTIFACT: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";
const ARCHIVE_RETENTION_DAYS: u64 = 7;
const TEST_STEP: &str = "test";

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE: &str = "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c";
const MISE_VERSION: &str = "2026.11.1";
const A_NEED_FAILED: &str =
    "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}";

pub(crate) fn rendered_job(job: &Job) -> RenderedJob {
    RenderedJob {
        id: job.id.to_string(),
        name: job.name.clone(),
        comment: job.comment.clone(),
        needs: job.needs.iter().map(ToString::to_string).collect(),
        condition: condition(job.when, job.gate.as_ref()),
        runs_on: job.runs_on.as_str().to_owned(),
        permissions: job.permissions.clone(),
        timeout_minutes: job.timeout_minutes,
        outputs: job
            .plan_outputs
            .iter()
            .map(|(slug, need)| {
                let key = output(slug, *need);
                let value = format!("${{{{ steps.{PLAN_STEP}.outputs.{key} }}}}");
                (key, value)
            })
            .chain(job.steps.iter().any(|step| {
                matches!(&step.kind, StepKind::Cargo(command) if reports_tests(command))
            }).then(|| {
                (
                    TESTS_RUN_OUTPUT.to_owned(),
                    format!("${{{{ steps.{TEST_STEP}.outputs.{TESTS_RUN_OUTPUT} }}}}"),
                )
            }))
            .collect(),
        steps: job.steps.iter().map(rendered_step).collect(),
    }
}

fn rendered_step(step: &Step) -> RenderedStep {
    let mut rendered = step_body(&step.kind);
    if let Some(condition) = condition(step.when, step.gate.as_ref()) {
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
        StepKind::InstallTools { only } => {
            let mut with = vec![
                ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
                ("cache".to_owned(), Value::Bool(false)),
            ];
            if let Some(tool) = only {
                with.push(("install_args".to_owned(), Value::Text(tool.clone())));
            }
            RenderedStep {
                name: "Install tools".to_owned(),
                uses: Some(MISE.to_owned()),
                with,
                ..RenderedStep::default()
            }
        }
        StepKind::UploadArchive { name, file } => upload_step(name, file),
        StepKind::DownloadArchive { name, into } => download_step(name, into.as_ref()),
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
        StepKind::Helper(command) => helper_step(*command),
        StepKind::GeneratorCheck => RenderedStep {
            name: "Check the generated workflow".to_owned(),
            run: Some("velnor-gen check".to_owned()),
            ..RenderedStep::default()
        },
        StepKind::InstallHelper(release) => install_helper(release),
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
                "cargo-nextest nextest run --workspace --locked --no-fail-fast --ignore-default-filter -E '{}' --no-tests=pass",
                filter_text(*filter)
            ),
            vec![],
        ),
        CargoCommand::NextestRunArchive { file, filter } => (
            "Test".into(),
            format!(
                "cargo-nextest nextest run --archive-file '{}' --extract-to . --extract-overwrite --workspace-remap . --no-fail-fast --ignore-default-filter -E '{}' --no-tests=pass",
                file.as_str().replace('\'', "'\\''"),
                filter_text(*filter)
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
        id: reports_tests(command).then(|| TEST_STEP.to_owned()),
        env,
        run: Some(format!("{RUN} {run}")),
        ..RenderedStep::default()
    }
}

fn filter_text(filter: FilterRef) -> &'static str {
    match filter {
        FilterRef::All => "all()",
    }
}

fn reports_tests(command: &CargoCommand) -> bool {
    matches!(
        command,
        CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. }
    )
}

fn helper_arguments(command: HelperCommand) -> &'static str {
    match command {
        HelperCommand::Plan => "plan",
        HelperCommand::Prepare => "prepare",
        HelperCommand::Restored => "restored",
        HelperCommand::InputsSave => "inputs save",
        HelperCommand::InputsRestore => "inputs restore",
    }
}

fn install_helper(release: &HelperRelease) -> RenderedStep {
    let version = release.version();
    let mut with = vec![("version".to_owned(), Value::Text(version.to_string()))];
    with.extend(release.digests().iter().map(|(platform, digest)| {
        (
            format!("sha256-{}", platform.file_suffix()),
            Value::Text(digest.to_string()),
        )
    }));
    RenderedStep {
        name: format!("Install velnor-ci {version}"),
        uses: Some(format!("{SETUP_VELNOR_ACTION}@{}", release.action())),
        uses_comment: Some(format!("rust-ci-v{version}")),
        with,
        ..RenderedStep::default()
    }
}

fn upload_step(name: &str, file: &RepoPath) -> RenderedStep {
    RenderedStep {
        name: "Upload the tests".to_owned(),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(name.to_owned())),
            ("path".to_owned(), Value::Text(file.to_string())),
            ("compression-level".to_owned(), Value::Int(0)),
            (
                "retention-days".to_owned(),
                Value::Int(ARCHIVE_RETENTION_DAYS),
            ),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
        ],
        ..RenderedStep::default()
    }
}

fn download_step(name: &str, into: Option<&RepoPath>) -> RenderedStep {
    let mut with = vec![("name".to_owned(), Value::Text(name.to_owned()))];
    if let Some(into) = into {
        with.push(("path".to_owned(), Value::Text(into.to_string())));
    }
    RenderedStep {
        name: "Download the tests".to_owned(),
        uses: Some(DOWNLOAD_ARTIFACT.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

fn helper_step(command: HelperCommand) -> RenderedStep {
    let arguments = helper_arguments(command);
    let (id, env) = match command {
        HelperCommand::Plan => (
            Some(PLAN_STEP),
            vec![(
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            )],
        ),
        HelperCommand::Prepare => (Some(PREPARE_STEP), Vec::new()),
        _ => (None, Vec::new()),
    };
    RenderedStep {
        name: format!("velnor-ci {arguments}"),
        id: id.map(str::to_owned),
        env,
        run: Some(format!("velnor-ci {arguments}")),
        ..RenderedStep::default()
    }
}
