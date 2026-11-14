// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{
    CargoCommand, FilterRef, RepoFacts, RepoPath, RepoView, RustMember, Shape, StepKind, Target,
    TargetKind, TargetName, Timings, When,
};
use velnor_rust::{CONFIG_PATH, DetectError, TIMINGS_PATH, decide_shape, plan};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use crate::select::{facts, member, name};

fn timings(lint: u64, test: u64, tests: u64) -> Timings {
    Timings {
        lint_build_seconds: lint,
        test_build_seconds: test,
        test_seconds: tests,
        test_binary_seconds: BTreeMap::new(),
    }
}

#[test]
fn the_shape_follows_the_config_and_otherwise_the_shorter_of_the_two_builds() {
    let cases = [
        (None, None, Shape::OneJob),
        (None, Some(timings(6, 30, 20)), Shape::LintAndTest),
        (None, Some(timings(30, 6, 20)), Shape::LintAndTest),
        (None, Some(timings(5, 300, 20)), Shape::OneJob),
        (None, Some(timings(300, 2, 20)), Shape::OneJob),
        (
            Some(Shape::OneJob),
            Some(timings(300, 300, 20)),
            Shape::OneJob,
        ),
        (Some(Shape::LintAndTest), None, Shape::LintAndTest),
        (Some(Shape::BuildThenGroups), None, Shape::BuildThenGroups),
    ];
    for (configured, measured, shape) in cases {
        assert_eq!(
            decide_shape(configured, measured.as_ref()).shape,
            shape,
            "{configured:?} {measured:?}"
        );
    }
}

#[test]
fn without_a_measured_shard_threshold_long_tests_stay_in_the_shared_job() {
    for (lint, test, shape) in [(2, 2, Shape::OneJob), (30, 30, Shape::LintAndTest)] {
        let decision = decide_shape(None, Some(&timings(lint, test, 100_000)));
        assert_eq!(decision.shape, shape);
    }
}

fn target(kind: TargetKind) -> Target {
    Target {
        name: TargetName::new("t").unwrap_or_else(|e| unreachable!("{e}")),
        kinds: vec![kind],
        tests: true,
        doctest: kind == TargetKind::Lib,
        source: None,
        requires_features: false,
    }
}

fn with_targets(package: &str, kinds: &[TargetKind]) -> RustMember {
    RustMember {
        targets: kinds.iter().map(|kind| target(*kind)).collect(),
        ..member(package, &format!("crates/{package}"), &[])
    }
}

fn crates(count: usize) -> Vec<RustMember> {
    (0..count)
        .map(|index| with_targets(&format!("c{index:03}"), &[TargetKind::Lib]))
        .collect()
}

fn job_ids(facts: &RepoFacts) -> Vec<String> {
    plan(facts)
        .jobs
        .iter()
        .map(|job| job.id.to_string())
        .collect()
}

#[test]
fn the_job_ids_do_not_grow_with_the_number_of_crates() {
    for (shape, ids) in [
        (None, vec!["rust", "required"]),
        (
            Some(Shape::LintAndTest),
            vec!["rust-lint", "rust-test", "required"],
        ),
    ] {
        let mut small = facts(vec![("Cargo.toml", crates(3))]);
        let mut large = facts(vec![("Cargo.toml", crates(150))]);
        small.config.shape = shape;
        large.config.shape = shape;
        assert_eq!(job_ids(&small), ids);
        assert_eq!(job_ids(&large), ids);
    }
}

fn cargo_steps(facts: &RepoFacts) -> Vec<(CargoCommand, When, Option<RepoPath>)> {
    plan(facts).jobs[0]
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Cargo(command) => Some((command.clone(), step.when, step.directory.clone())),
            _ => None,
        })
        .collect()
}

fn alone(package: &str) -> CargoCommand {
    CargoCommand::CheckAlone {
        package: name(package),
    }
}

#[test]
fn the_single_job_runs_each_check_once_in_order_then_each_crate_with_a_library_or_binary_alone()
-> TestResult {
    let members = vec![
        with_targets("bin", &[TargetKind::Bin, TargetKind::Test]),
        with_targets("examples", &[TargetKind::Example, TargetKind::Bench]),
        with_targets("lib", &[TargetKind::Lib]),
        with_targets("macro", &[TargetKind::ProcMacro]),
        with_targets("tests-only", &[TargetKind::Test]),
        RustMember {
            targets: vec![Target {
                requires_features: true,
                ..target(TargetKind::Bin)
            }],
            ..member("gated", "crates/gated", &[])
        },
    ];
    let steps = cargo_steps(&facts(vec![("tools/Cargo.toml", members)]));
    let commands: Vec<CargoCommand> = steps.iter().map(|(c, _, _)| c.clone()).collect();
    assert_eq!(
        commands,
        [
            CargoCommand::FmtCheck,
            CargoCommand::Clippy { extra: Vec::new() },
            CargoCommand::NextestRun {
                filter: FilterRef::All
            },
            CargoCommand::Doctest,
            CargoCommand::Doc,
            alone("bin"),
            alone("lib"),
            alone("macro"),
        ]
    );
    let directory = Some(RepoPath::new("tools")?);
    assert!(
        steps
            .iter()
            .all(|(_, when, dir)| *when == When::NotCancelled && *dir == directory)
    );

    let binaries = vec![with_targets("bin", &[TargetKind::Bin])];
    let commands: Vec<CargoCommand> = cargo_steps(&facts(vec![("Cargo.toml", binaries)]))
        .into_iter()
        .map(|(c, _, _)| c)
        .collect();
    assert!(!commands.contains(&CargoCommand::Doctest), "{commands:?}");

    let untested = RustMember {
        targets: vec![Target {
            tests: false,
            ..target(TargetKind::Bin)
        }],
        ..member("untested", "crates/untested", &[])
    };
    let mut no_tests = facts(vec![("Cargo.toml", vec![untested])]);
    let commands: Vec<CargoCommand> = cargo_steps(&no_tests)
        .into_iter()
        .map(|(c, _, _)| c)
        .collect();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, CargoCommand::NextestRun { .. })),
        "{commands:?}"
    );
    no_tests.config.shape = Some(Shape::LintAndTest);
    assert_eq!(job_ids(&no_tests), ["rust-lint", "required"]);
    Ok(())
}

fn timed_view(timings: &str, config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    view.tracked_paths.insert(TIMINGS_PATH);
    view.files.insert(TIMINGS_PATH, timings.as_bytes().to_vec());
    Ok(view)
}

#[test]
fn a_committed_timings_entry_splits_only_its_own_root_and_the_config_names_the_verdict()
-> TestResult {
    let entry = r#"{"lab/Cargo.toml": {"lint_build_seconds": 10, "test_build_seconds": 20, "test_seconds": 4}}"#;
    let text = workflow(&timed_view(entry, "required_check = \"CI passed\"\n")?)?;
    let jobs: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .collect();
    assert_eq!(
        jobs,
        [
            "  rust-root:",
            "  rust-lint-lab:",
            "  rust-test-lab:",
            "  required:"
        ]
    );
    assert!(text.contains("  # lint and test jobs: the shorter build takes 10 s, longer than the 5 s a second job adds (#585)\n  rust-lint-lab:\n"), "{text}");
    assert!(text.contains("    name: CI passed\n"), "{text}");
    assert_eq!(text.matches("      - name: Install tools\n").count(), 3);
    assert!(!text.contains("install_args"), "{text}");
    let components: Vec<usize> = ["  rust-root:", "  rust-lint-lab:", "  rust-test-lab:"]
        .iter()
        .map(|job| {
            let start = text.find(job).unwrap_or(0);
            let end = text[start + 1..]
                .find("\n  rust-")
                .or_else(|| text[start + 1..].find("\n  required:"))
                .map_or(text.len(), |at| start + 1 + at);
            text[start..end]
                .matches("run: rustup component add clippy rustfmt")
                .count()
        })
        .collect();
    assert_eq!(components, [1, 1, 0]);

    let invalid = |path: RepoPath| {
        move |result: Result<BTreeMap<RepoPath, String>, GenerateError>| matches!(result, Err(GenerateError::Detect(DetectError::Invalid { path: p, .. })) if p == path)
    };
    for timings in [
        "{",
        r#"{"lab/Cargo.toml": {"lint_build_seconds": 1, "test_build_seconds": 1}}"#,
        r#"{"lab/Cargo.toml": {"lint_build_seconds": 1, "test_build_seconds": 1, "test_seconds": 1, "x": 1}}"#,
        r#"{"gone/Cargo.toml": {"lint_build_seconds": 1, "test_build_seconds": 1, "test_seconds": 1}}"#,
    ] {
        let result = expected_files(&timed_view(timings, "")?);
        assert!(
            invalid(TIMINGS_PATH)(result.clone()),
            "{timings}: {result:?}"
        );
    }
    for config in [
        "required_check = \"rust\"\n",
        "shape = \"two\"\n",
        "required_check = \"\"\n",
        "required_check = \"a\\u0007b\"\n",
    ] {
        let result = expected_files(&view(&["Cargo.toml"], Some(config.as_bytes()))?);
        assert!(invalid(CONFIG_PATH)(result.clone()), "{config}: {result:?}");
    }
    Ok(())
}
