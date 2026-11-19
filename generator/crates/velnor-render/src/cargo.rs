// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUILD_TESTS_STEP, CargoCommand, FilterRef, LINT_STEP, RepoPath, TEST_STEP, TargetKind,
    TargetName,
};

use crate::gate::CACHE_DEBUG_ON;
use crate::rules::{RenderedStep, Value};
use crate::steps::{RUN, RUN_DOCTESTS, TEST_STEP_ID};

const FINGERPRINT_LOG: &str =
    "cargo::compiler::fingerprint=info,cargo::core::compiler::fingerprint=info";
const DOC_RUN: &str =
    "cargo doc --workspace --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'";

#[derive(Clone, Copy)]
pub(crate) struct CargoLane {
    pub(crate) artifacts: ArtifactFormat,
    pub(crate) cache_debug: bool,
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

pub(crate) fn cargo_step(command: &CargoCommand, audit: bool, lane: CargoLane) -> RenderedStep {
    let (name, run, env) = cargo_command(command, lane.artifacts);
    let prefix = if matches!(command, CargoCommand::Doctest) {
        RUN_DOCTESTS
    } else {
        RUN
    };
    RenderedStep {
        name,
        id: reports_tests(command).then(|| TEST_STEP_ID.to_owned()),
        env: env
            .into_iter()
            .chain(lane.cache_debug.then(cache_debug_env))
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
) -> (String, String, Vec<(String, Value)>) {
    let cargo = artifacts.cargo();
    let nextest = artifacts.nextest();
    match command {
        CargoCommand::FmtCheck => ("Format".into(), "cargo-fmt --all --check".into(), vec![]),
        CargoCommand::Clippy { extra } => clippy(cargo, extra),
        CargoCommand::NextestBuild => nextest_build(nextest),
        CargoCommand::NextestRun { filter, ci_profile } => nextest_run(
            &format!("--workspace --locked{nextest}"),
            filter,
            *ci_profile,
        ),
        CargoCommand::NextestFeatures {
            features,
            filter,
            ci_profile,
        } => nextest_features(nextest, features, filter, *ci_profile),
        CargoCommand::NextestRunArchive {
            file,
            filter,
            ci_profile,
        } => nextest_run(
            &format!(
                "--archive-file '{}' --extract-to . --extract-overwrite --workspace-remap .",
                file.as_str().replace('\'', "'\\''")
            ),
            filter,
            *ci_profile,
        ),
        CargoCommand::NextestArchive { file } => (
            BUILD_TESTS_STEP.into(),
            format!(
                "cargo-nextest nextest archive --workspace --locked{nextest} --archive-file '{}'",
                file.as_str().replace('\'', "'\\''")
            ),
            vec![],
        ),
        CargoCommand::TestTarget {
            package,
            target,
            kind,
            features,
        } => (
            format!("Test {package} {target}"),
            format!(
                "cargo test -p {package} --locked{cargo} {}{}",
                test_selector(*kind, target),
                feature_flag(features)
            ),
            vec![],
        ),
        CargoCommand::Doctest => (
            "Doctests".into(),
            format!("cargo test --doc --workspace --locked{cargo}"),
            vec![],
        ),
        CargoCommand::Doc => ("Documentation".into(), format!("{DOC_RUN}{cargo}"), vec![]),
        CargoCommand::CheckAlone { package } => (
            format!("Check {package} alone"),
            format!("cargo check -p {package} --locked{cargo}"),
            vec![],
        ),
        CargoCommand::BuildBinary { package, binary } => (
            format!("Build {package}"),
            format!("cargo build -p {package} --bin {binary} --locked{cargo}"),
            vec![],
        ),
        CargoCommand::BuildAll => (
            "Build every target".into(),
            format!("cargo build --workspace --all-targets --locked{cargo}"),
            vec![],
        ),
        CargoCommand::Deny {
            manifest,
            locked,
            config,
        } => deny(manifest, *locked, config.as_ref()),
        CargoCommand::Machete => ("Unused dependencies".into(), "cargo-machete".into(), vec![]),
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
        )
    {
        "velnor-ci inputs build -- "
    } else {
        ""
    }
}

fn clippy(cargo: &str, extra: &[String]) -> (String, String, Vec<(String, Value)>) {
    (
        LINT_STEP.into(),
        [format!(
            "cargo-clippy clippy --workspace --all-targets --locked{cargo} -- -D warnings"
        )]
        .into_iter()
        .chain(extra.iter().cloned())
        .collect::<Vec<_>>()
        .join(" "),
        vec![],
    )
}

fn cache_debug_env() -> (String, Value) {
    (
        "CARGO_LOG".to_owned(),
        Value::Expr(format!(
            "${{{{ {CACHE_DEBUG_ON} && '{FINGERPRINT_LOG}' || '' }}}}"
        )),
    )
}

fn nextest_build(format: &str) -> (String, String, Vec<(String, Value)>) {
    (
        BUILD_TESTS_STEP.into(),
        format!(
            "cargo-nextest nextest run --workspace --locked{format} --ignore-default-filter --no-run"
        ),
        vec![],
    )
}

fn nextest_run(
    scope: &str,
    filter: &FilterRef,
    ci_profile: bool,
) -> (String, String, Vec<(String, Value)>) {
    let profile = if ci_profile { " --profile ci" } else { "" };
    (
        TEST_STEP.into(),
        format!(
            "cargo-nextest nextest run {scope}{profile} --no-fail-fast --ignore-default-filter --status-level pass -E \"{}\" --no-tests=pass",
            nextest_filter(filter)
        ),
        vec![],
    )
}

fn nextest_filter(filter: &FilterRef) -> String {
    match filter {
        FilterRef::All => "$TEST_FILTER".to_owned(),
        FilterRef::AllExcept(binaries) => binaries
            .iter()
            .fold("($TEST_FILTER)".to_owned(), |text, id| {
                format!("{text} - binary_id(={id})")
            }),
        FilterRef::Expr { expr, except } => {
            let both = format!("($TEST_FILTER) and ({expr})");
            if except.is_empty() {
                both
            } else {
                except.iter().fold(format!("({both})"), |text, id| {
                    format!("{text} - binary_id(={id})")
                })
            }
        }
    }
}

fn nextest_features(
    nextest: &str,
    features: &[String],
    filter: &FilterRef,
    ci_profile: bool,
) -> (String, String, Vec<(String, Value)>) {
    let (_, run, env) = nextest_run(
        &format!("--workspace --locked{nextest}{}", feature_flag(features)),
        filter,
        ci_profile,
    );
    (
        format!("{TEST_STEP} (features: {})", features.join(",")),
        run,
        env,
    )
}

fn feature_flag(features: &[String]) -> String {
    if features.is_empty() {
        String::new()
    } else {
        format!(
            " --features '{}'",
            features.join(",").replace('\'', "'\\''")
        )
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

fn deny(
    manifest: &RepoPath,
    locked: bool,
    config: Option<&RepoPath>,
) -> (String, String, Vec<(String, Value)>) {
    let quoted = |path: &RepoPath| path.as_str().replace('\'', "'\\''");
    let locked = if locked { " --locked" } else { "" };
    let config = config.map_or_else(String::new, |path| format!(" --config '{}'", quoted(path)));
    match manifest.parent() {
        None => (
            "Dependency policy".into(),
            format!("cargo-deny{locked}{config} check"),
            vec![],
        ),
        Some(directory) => (
            format!("Dependency policy ({directory})"),
            format!(
                "cargo-deny{locked} --manifest-path '{}'{config} check",
                quoted(manifest)
            ),
            vec![],
        ),
    }
}

pub(crate) fn reports_tests(command: &CargoCommand) -> bool {
    matches!(
        command,
        CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. }
    )
}
