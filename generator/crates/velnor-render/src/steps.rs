// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CargoCommand, FilterRef, GateNeed, HelperCommand, HelperRelease, Job, LINT_STEP,
    RepoPath, Step, StepKind, TEST_STEP, TESTS_RUN_OUTPUT, TargetKind, TargetName,
};

use crate::cache::{path_text, render_key};
use crate::gate::{PLAN_STEP, condition, output, plan_source};
use crate::permission_pairs;
use crate::rules::{
    HELPER_FROM_ARTIFACT_RUN, RenderedJob, RenderedStep, SETUP_VELNOR_ACTION, Value,
};

pub(crate) const PREPARE_STEP: &str = "prepare";
const RESTORED_STEP: &str = "restored";
const RUN: &str = "velnor-ci run --";
const RUN_DOCTESTS: &str = "velnor-ci run --require-doctests=$DOCTESTS_KNOWN --";

pub(crate) const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
pub(crate) const MISE: &str = "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c";
const UPLOAD_ARTIFACT: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";
const HELPER_ARTIFACT: &str = "velnor-ci";
const ARCHIVE_RETENTION_DAYS: u64 = 7;
const TEST_STEP_ID: &str = "test";
pub(crate) const MISE_VERSION: &str = "2026.11.1";
const IS_DRAFT: &str = "${{ github.event.pull_request.draft == true }}";
const CACHE_RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const CACHE_SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const A_NEED_FAILED: &str =
    "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}";

pub(crate) fn rendered_job(job: &Job) -> RenderedJob {
    RenderedJob {
        id: job.id.to_string(),
        name: job.name.clone(),
        comment: job.comment.clone(),
        needs: job.needs.iter().map(ToString::to_string).collect(),
        condition: condition(job.when, job.gate.as_ref(), false, job.skip_on_draft),
        runs_on: job.runs_on.as_str().to_owned(),
        permissions: permission_pairs(&job.permissions),
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
                    format!("${{{{ steps.{TEST_STEP_ID}.outputs.{TESTS_RUN_OUTPUT} }}}}"),
                )
            }))
            .collect(),
        steps: job.steps.iter().flat_map(rendered_steps).collect(),
        ..RenderedJob::default()
    }
}

fn rendered_steps(step: &Step) -> Vec<RenderedStep> {
    let mut rendered = vec![step_body(&step.kind)];
    if step.kind == StepKind::HelperFromArtifact {
        rendered.push(RenderedStep {
            name: "Make velnor-ci executable".to_owned(),
            run: Some(HELPER_FROM_ARTIFACT_RUN.to_owned()),
            ..RenderedStep::default()
        });
    }
    for body in &mut rendered {
        if let Some(condition) = condition(step.when, step.gate.as_ref(), true, step.skip_on_draft)
        {
            body.condition = Some(condition);
        }
        body.working_directory = step.directory.as_ref().map(ToString::to_string);
    }
    if let (Some(plan), StepKind::Cargo(command), Some(main)) =
        (&step.reads, &step.kind, rendered.first_mut())
    {
        let source = plan_source(&plan.from);
        main.env.extend(command.plan_values().iter().map(|need| {
            let key = output(&plan.slug, *need);
            (
                variable(*need).to_owned(),
                Value::Expr(format!("${{{{ {source}.{key} }}}}")),
            )
        }));
    }
    rendered
}

fn variable(need: GateNeed) -> &'static str {
    match need {
        GateNeed::TestFilter => "TEST_FILTER",
        GateNeed::DoctestsKnown => "DOCTESTS_KNOWN",
        _ => "PACKAGE_ARGS",
    }
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
        StepKind::CacheRestore(key) => cache_step(CACHE_RESTORE, "Restore", key, true),
        StepKind::CacheSave(key) => cache_step(CACHE_SAVE, "Save", key, false),
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
        StepKind::FailOnDraftRun => RenderedStep {
            name: "Fail on a draft pull request, whose jobs were skipped".to_owned(),
            condition: Some(IS_DRAFT.to_owned()),
            run: Some("exit 1".to_owned()),
            ..RenderedStep::default()
        },
        StepKind::HelperFromArtifact => download_helper(),
        StepKind::PublishHelper { binary } => publish_helper(binary),
        StepKind::FailWhenANeedFailed => RenderedStep {
            name: "Fail when a Rust job failed or was cancelled".to_owned(),
            condition: Some(A_NEED_FAILED.to_owned()),
            run: Some("exit 1".to_owned()),
            ..RenderedStep::default()
        },
    }
}

fn download_helper() -> RenderedStep {
    RenderedStep {
        name: "Download velnor-ci".to_owned(),
        uses: Some(DOWNLOAD_ARTIFACT.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(HELPER_ARTIFACT.to_owned())),
            (
                "path".to_owned(),
                Value::Expr("${{ runner.temp }}/vbin".to_owned()),
            ),
        ],
        ..RenderedStep::default()
    }
}

fn publish_helper(binary: &RepoPath) -> RenderedStep {
    RenderedStep {
        name: "Upload velnor-ci".to_owned(),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(HELPER_ARTIFACT.to_owned())),
            ("path".to_owned(), Value::Text(binary.to_string())),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
            ("retention-days".to_owned(), Value::Int(1)),
            ("overwrite".to_owned(), Value::Bool(true)),
        ],
        ..RenderedStep::default()
    }
}

fn cache_step(action: &str, verb: &str, key: &CacheKey, fallbacks: bool) -> RenderedStep {
    let rendered = render_key(key);
    let paths: Vec<String> = key.paths().iter().map(path_text).collect();
    let mut with = vec![
        ("path".to_owned(), Value::Text(paths.join("\n"))),
        ("key".to_owned(), Value::Expr(rendered.key)),
    ];
    if fallbacks && !rendered.restore_keys.is_empty() {
        with.push((
            "restore-keys".to_owned(),
            Value::Expr(rendered.restore_keys.join("\n")),
        ));
    }
    RenderedStep {
        name: format!("{verb} the {} cache", key.family()),
        uses: Some(action.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

fn cargo_step(command: &CargoCommand) -> RenderedStep {
    let (name, run, env): (String, String, Vec<(String, Value)>) = match command {
        CargoCommand::FmtCheck => ("Format".into(), "cargo-fmt --all --check".into(), vec![]),
        CargoCommand::Clippy { extra } => (
            LINT_STEP.into(),
            ["cargo-clippy clippy $PACKAGE_ARGS --all-targets --locked -- -D warnings"]
                .into_iter()
                .chain(extra.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            vec![],
        ),
        CargoCommand::NextestRun { filter } => (
            TEST_STEP.into(),
            format!(
                "cargo-nextest nextest run $PACKAGE_ARGS --locked --no-fail-fast --ignore-default-filter -E \"{}\" --no-tests=pass",
                nextest_filter(filter)
            ),
            vec![],
        ),
        CargoCommand::NextestRunArchive { file, filter } => (
            TEST_STEP.into(),
            format!(
                "cargo-nextest nextest run --archive-file '{}' --extract-to . --extract-overwrite --workspace-remap . --no-fail-fast --ignore-default-filter -E \"{}\" --no-tests=pass",
                file.as_str().replace('\'', "'\\''"),
                nextest_filter(filter)
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
        CargoCommand::TestTarget {
            package,
            target,
            kind,
        } => (
            format!("Test {package} {target}"),
            format!(
                "cargo test -p {package} --locked {}",
                test_selector(*kind, target)
            ),
            vec![],
        ),
        CargoCommand::Doctest => (
            "Doctests".into(),
            "cargo test --doc $PACKAGE_ARGS --locked".into(),
            vec![],
        ),
        CargoCommand::Doc => (
            "Documentation".into(),
            "cargo doc $PACKAGE_ARGS --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'"
                .into(),
            vec![],
        ),
        CargoCommand::CheckAlone { package } => (
            format!("Check {package} alone"),
            format!("cargo check -p {package} --locked"),
            vec![],
        ),
        CargoCommand::Build { package } => (
            format!("Build {package}"),
            format!("cargo build -p {package} --locked"),
            vec![],
        ),
        CargoCommand::Deny { manifest } => deny(manifest),
        CargoCommand::Machete => ("Unused dependencies".into(), "cargo-machete".into(), vec![]),
    };
    let prefix = if matches!(command, CargoCommand::Doctest) {
        RUN_DOCTESTS
    } else {
        RUN
    };
    RenderedStep {
        name,
        id: reports_tests(command).then(|| TEST_STEP_ID.to_owned()),
        env,
        run: Some(format!("{prefix} {run}")),
        ..RenderedStep::default()
    }
}

fn nextest_filter(filter: &FilterRef) -> String {
    match filter {
        FilterRef::All => "$TEST_FILTER".to_owned(),
        FilterRef::AllExcept(binaries) => binaries
            .iter()
            .fold("($TEST_FILTER)".to_owned(), |text, id| {
                format!("{text} - binary_id(={id})")
            }),
    }
}

fn test_selector(kind: TargetKind, target: &TargetName) -> String {
    match kind {
        TargetKind::Lib | TargetKind::ProcMacro => "--lib".to_owned(),
        TargetKind::Bin => format!("--bin={target}"),
        TargetKind::Bench => format!("--bench={target}"),
        TargetKind::Example => format!("--example={target}"),
        TargetKind::Test | TargetKind::BuildScript => format!("--test={target}"),
    }
}

fn deny(manifest: &RepoPath) -> (String, String, Vec<(String, Value)>) {
    match manifest.parent() {
        None => (
            "Dependency policy".into(),
            "cargo-deny check".into(),
            vec![],
        ),
        Some(directory) => (
            format!("Dependency policy ({directory})"),
            format!(
                "cargo-deny --manifest-path '{}' check",
                manifest.as_str().replace('\'', "'\\''")
            ),
            vec![],
        ),
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
        HelperCommand::CheckMetadata => "check-metadata",
        HelperCommand::Restored => "restored",
        HelperCommand::InputsSave => "inputs save",
        HelperCommand::InputsRestore => "inputs restore",
        HelperCommand::InputsAudit => "inputs audit",
    }
}

pub(crate) fn install_helper(release: &HelperRelease) -> RenderedStep {
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
        HelperCommand::Plan => (Some(PLAN_STEP), Vec::new()),
        HelperCommand::Prepare => (Some(PREPARE_STEP), Vec::new()),
        HelperCommand::Restored => (Some(RESTORED_STEP), Vec::new()),
        _ => (None, Vec::new()),
    };
    RenderedStep {
        name: format!("velnor-ci {arguments}"),
        id: id.map(str::to_owned),
        env,
        run: Some(match command {
            HelperCommand::Plan => format!("velnor-ci {arguments} --explain"),
            _ => format!("velnor-ci {arguments}"),
        }),
        ..RenderedStep::default()
    }
}
