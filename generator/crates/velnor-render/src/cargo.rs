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

pub(crate) fn cargo_step(command: &CargoCommand, audit: bool) -> RenderedStep {
    let (name, run, env): (String, String, Vec<(String, Value)>) = match command {
        CargoCommand::FmtCheck => ("Format".into(), "cargo-fmt --all --check".into(), vec![]),
        CargoCommand::Clippy { extra } => (
            LINT_STEP.into(),
            ["cargo-clippy clippy --workspace --all-targets --locked -- -D warnings"]
                .into_iter()
                .chain(extra.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            vec![],
        ),
        CargoCommand::NextestRun { filter } => nextest_run("--workspace --locked", filter),
        CargoCommand::NextestRunArchive { file, filter } => nextest_run(
            &format!(
                "--archive-file '{}' --extract-to . --extract-overwrite --workspace-remap .",
                file.as_str().replace('\'', "'\\''")
            ),
            filter,
        ),
        CargoCommand::NextestArchive { file } => (
            BUILD_TESTS_STEP.into(),
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
            features,
        } => (
            format!("Test {package} {target}"),
            format!(
                "cargo test -p {package} --locked {}{}",
                test_selector(*kind, target),
                feature_flag(features)
            ),
            vec![],
        ),
        CargoCommand::Doctest => (
            "Doctests".into(),
            "cargo test --doc --workspace --locked".into(),
            vec![],
        ),
        CargoCommand::Doc => ("Documentation".into(), DOC_RUN.into(), vec![]),
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
        CargoCommand::BuildAll => (
            "Build every target".into(),
            "cargo build --workspace --all-targets --locked".into(),
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
    let inventory = inventory_prefix(command, audit);
    RenderedStep {
        name,
        id: reports_tests(command).then(|| TEST_STEP_ID.to_owned()),
        env: env.into_iter().chain([cache_debug_env()]).collect(),
        run: Some(format!("{prefix} {inventory}{run}")),
        ..RenderedStep::default()
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

fn cache_debug_env() -> (String, Value) {
    (
        "CARGO_LOG".to_owned(),
        Value::Expr(format!(
            "${{{{ {CACHE_DEBUG_ON} && '{FINGERPRINT_LOG}' || '' }}}}"
        )),
    )
}

fn nextest_run(scope: &str, filter: &FilterRef) -> (String, String, Vec<(String, Value)>) {
    (
        TEST_STEP.into(),
        format!(
            "cargo-nextest nextest run {scope} --no-fail-fast --ignore-default-filter --status-level pass -E \"{}\" --no-tests=pass",
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
    }
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

pub(crate) fn reports_tests(command: &CargoCommand) -> bool {
    matches!(
        command,
        CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. }
    )
}
