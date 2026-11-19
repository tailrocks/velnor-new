// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUILD_TESTS_STEP, CargoCommand, FilterRef, Job, LINT_STEP, RepoPath, StepKind, TEST_STEP,
    TargetKind, TargetName,
};

use crate::cargo_features::NO_TESTS_PASS;
use crate::cargo_path::{manifest_flag, nextest_archive, test_target};
pub(crate) use crate::cargo_target::{hoist_target_dir, target_dir_env};
use crate::gate::CACHE_DEBUG_ON;
use crate::rules::{RenderedStep, Value};
use crate::steps::{RUN, RUN_DOCTESTS};

const FINGERPRINT_LOG: &str =
    "cargo::compiler::fingerprint=info,cargo::core::compiler::fingerprint=info";
pub(crate) const DOC_RUN: &str =
    "cargo doc --workspace --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'";

#[derive(Clone, Copy)]
pub(crate) struct CargoLane {
    pub(crate) artifacts: ArtifactFormat,
    pub(crate) cache_debug: bool,
    pub(crate) offline: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ArtifactFormat {
    Disabled,
    CargoOnly,
    All,
}

impl ArtifactFormat {
    const fn cargo(self) -> &'static str {
        match self {
            Self::Disabled => "",
            Self::CargoOnly | Self::All => " --message-format=json-render-diagnostics",
        }
    }

    const fn nextest(self) -> &'static str {
        match self {
            Self::Disabled | Self::CargoOnly => "",
            Self::All => " --cargo-message-format=json-render-diagnostics",
        }
    }
}

pub(crate) fn cargo_lane(job: &Job) -> CargoLane {
    let artifacts = job
        .steps
        .iter()
        .find_map(|step| match step.kind {
            StepKind::ReportCaches {
                nextest_artifacts, ..
            } => Some(if nextest_artifacts {
                ArtifactFormat::All
            } else {
                ArtifactFormat::CargoOnly
            }),
            _ => None,
        })
        .unwrap_or(ArtifactFormat::Disabled);
    CargoLane {
        artifacts,
        cache_debug: job
            .steps
            .iter()
            .any(|step| step.kind == StepKind::UploadCargoLog),
        offline: job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::PrivateFetch { .. })),
    }
}

pub(crate) fn cargo_step(
    command: &CargoCommand,
    manifest: Option<&RepoPath>,
    audit: bool,
    lane: CargoLane,
    test_id: Option<String>,
) -> RenderedStep {
    let (name, run, env) = cargo_command(command, lane.artifacts, manifest);
    let run = if lane.offline {
        run.replacen(" --locked", " --locked --offline", 1)
    } else {
        run
    };
    let prefix = if matches!(command, CargoCommand::Doctest) {
        RUN_DOCTESTS
    } else {
        RUN
    };
    RenderedStep {
        name,
        id: test_id,
        env: env
            .into_iter()
            .chain(lane.cache_debug.then(cache_debug_env))
            .chain(lane.offline.then(offline_env))
            .collect(),
        run: Some(format!(
            "{prefix} {}{run}",
            inventory_prefix(command, audit)
        )),
        ..RenderedStep::default()
    }
}

fn cargo_command(
    command: &CargoCommand,
    artifacts: ArtifactFormat,
    manifest: Option<&RepoPath>,
) -> (String, String, Vec<(String, Value)>) {
    let cargo = artifacts.cargo();
    let nextest = artifacts.nextest();
    let path = manifest_flag(manifest);
    match command {
        CargoCommand::FmtCheck => (
            "Format".into(),
            format!("cargo-fmt{path} --all --check"),
            vec![],
        ),
        CargoCommand::Clippy { extra } => clippy(cargo, extra, &path),
        CargoCommand::ClippyFeatures { features } => {
            crate::cargo_features::clippy_features(cargo, features)
        }
        CargoCommand::NextestBuild => nextest_build(nextest, &path),
        CargoCommand::NextestRun { filter, ci_profile } => nextest_run(
            &format!("--workspace --locked{nextest}"),
            filter,
            *ci_profile,
            NO_TESTS_PASS,
            &path,
        ),
        CargoCommand::NextestFeatures {
            features,
            filter,
            ci_profile,
            ..
        } => crate::cargo_features::nextest_features(nextest, features, filter, *ci_profile),
        CargoCommand::LaneGuard {
            features,
            filter,
            except,
            ..
        } => crate::cargo_features::lane_guard(features, filter, except),
        CargoCommand::NextestRunArchive {
            file,
            filter,
            threads,
            ci_profile,
        } => nextest_run(
            &format!(
                "--archive-file '{}' --extract-to . --extract-overwrite --workspace-remap .{}",
                file.as_str().replace('\'', "'\\''"),
                threads.map_or_else(String::new, |count| format!(" --test-threads {count}"))
            ),
            filter,
            *ci_profile,
            NO_TESTS_PASS,
            &path,
        ),
        CargoCommand::NextestArchive { file } => nextest_archive(nextest, file, &path),
        CargoCommand::TestTarget {
            package,
            target,
            kind,
            features,
        } => test_target(cargo, package, target, *kind, features, &path),
        CargoCommand::Doctest => (
            "Doctests".into(),
            format!("cargo test{path} --doc --workspace --locked{cargo}"),
            vec![],
        ),
        CargoCommand::Doc => (
            "Documentation".into(),
            format!(
                "cargo doc{path} --workspace --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'{cargo}"
            ),
            vec![],
        ),
        CargoCommand::DocFeatures { features } => {
            crate::cargo_features::doc_features(cargo, features)
        }
        CargoCommand::CheckAlone { package } => (
            format!("Check {package} alone"),
            format!("cargo check{path} -p {package} --locked{cargo}"),
            vec![],
        ),
        CargoCommand::BuildBinary { package, binary } => (
            format!("Build {package}"),
            format!("cargo build{path} -p {package} --bin {binary} --locked{cargo}"),
            vec![],
        ),
        CargoCommand::BuildAll => (
            "Build every target".into(),
            format!("cargo build{path} --workspace --all-targets --locked{cargo}"),
            vec![],
        ),
        CargoCommand::Deny { manifest, config } => deny(manifest, config.as_ref()),
        CargoCommand::Machete => ("Unused dependencies".into(), "cargo machete".into(), vec![]),
    }
}

fn inventory_prefix(command: &CargoCommand, audit: bool) -> &'static str {
    if audit
        && !matches!(
            command,
            CargoCommand::FmtCheck
                | CargoCommand::Deny { .. }
                | CargoCommand::Machete
                | CargoCommand::NextestRunArchive { .. }
                | CargoCommand::LaneGuard { .. }
        )
    {
        "velnor-ci inputs build -- "
    } else {
        ""
    }
}

pub(crate) fn clippy(
    cargo: &str,
    extra: &[String],
    path: &str,
) -> (String, String, Vec<(String, Value)>) {
    (
        LINT_STEP.into(),
        [format!(
            "cargo-clippy clippy{path} --workspace --all-targets --locked{cargo} -- -D warnings"
        )]
        .into_iter()
        .chain(extra.iter().cloned())
        .collect::<Vec<_>>()
        .join(" "),
        vec![],
    )
}

pub(crate) fn cargo_fetch(lane: CargoLane) -> RenderedStep {
    let offline = if lane.offline { " --offline" } else { "" };
    RenderedStep {
        env: lane
            .cache_debug
            .then(cache_debug_env)
            .into_iter()
            .chain(lane.offline.then(offline_env))
            .collect(),
        run: Some(format!("{RUN} cargo fetch --locked{offline}")),
        ..RenderedStep::default()
    }
}

fn offline_env() -> (String, Value) {
    (
        "CARGO_NET_OFFLINE".to_owned(),
        Value::Text("true".to_owned()),
    )
}

pub(crate) fn cache_debug_env() -> (String, Value) {
    (
        "CARGO_LOG".to_owned(),
        Value::Expr(format!(
            "${{{{ {CACHE_DEBUG_ON} && '{FINGERPRINT_LOG}' || '' }}}}"
        )),
    )
}

fn nextest_build(format: &str, path: &str) -> (String, String, Vec<(String, Value)>) {
    (
        BUILD_TESTS_STEP.into(),
        format!(
            "cargo-nextest nextest run{path} --workspace --locked{format} --ignore-default-filter --no-run"
        ),
        vec![],
    )
}

pub(crate) fn nextest_run(
    scope: &str,
    filter: &FilterRef,
    ci_profile: bool,
    no_tests: &str,
    path: &str,
) -> (String, String, Vec<(String, Value)>) {
    let profile = if ci_profile { " --profile ci" } else { "" };
    (
        TEST_STEP.into(),
        format!(
            "cargo-nextest nextest run{path} {scope}{profile} --no-fail-fast --ignore-default-filter --status-level pass -E \"{}\" --no-tests={no_tests}",
            nextest_filter(filter)
        ),
        vec![],
    )
}

fn nextest_filter(filter: &FilterRef) -> String {
    match filter {
        FilterRef::All => "$TEST_FILTER".to_owned(),
        FilterRef::AllExcept(binaries) => without_binaries("($TEST_FILTER)".to_owned(), binaries),
        FilterRef::Expr { expr, except } => crate::cargo_features::expr(expr, except),
        FilterRef::Group {
            include,
            exclude,
            except,
        } => {
            let selected = include
                .iter()
                .fold("($TEST_FILTER)".to_owned(), |text, expr| {
                    format!("({text} and ({expr}))")
                });
            let owned = exclude
                .iter()
                .fold(selected, |text, expr| format!("({text} and not ({expr}))"));
            without_binaries(owned, except)
        }
    }
}

pub(crate) fn without_binaries(base: String, binaries: &[String]) -> String {
    binaries
        .iter()
        .fold(base, |text, id| format!("{text} - binary_id(={id})"))
}

pub(crate) fn feature_flag(features: &[String]) -> String {
    if features.is_empty() {
        String::new()
    } else {
        format!(
            " --features '{}'",
            features.join(",").replace('\'', "'\\''")
        )
    }
}

pub(crate) fn test_selector(kind: TargetKind, target: &TargetName) -> String {
    match kind {
        TargetKind::Lib | TargetKind::ProcMacro => "--lib".to_owned(),
        TargetKind::Bin => format!("--bin={target}"),
        TargetKind::Bench => format!("--bench={target}"),
        TargetKind::Example => format!("--example={target}"),
        TargetKind::Test | TargetKind::BuildScript => format!("--test={target}"),
    }
}

fn deny(manifest: &RepoPath, config: Option<&RepoPath>) -> (String, String, Vec<(String, Value)>) {
    let directory = manifest.parent();
    let name = directory.as_ref().map_or_else(
        || "Dependency policy".to_owned(),
        |directory| format!("Dependency policy ({directory})"),
    );
    let manifest_arg = directory.map_or_else(String::new, |_| {
        format!(
            " --manifest-path '{}'",
            manifest.as_str().replace('\'', "'\\''")
        )
    });
    let config_arg = config.map_or_else(String::new, |path| {
        format!(" --config '{}'", path.as_str().replace('\'', "'\\''"))
    });
    (
        name,
        format!("cargo-deny{manifest_arg}{config_arg} --locked check"),
        vec![],
    )
}

pub(crate) fn reports_tests(command: &CargoCommand) -> bool {
    matches!(
        command,
        CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. }
    )
}
