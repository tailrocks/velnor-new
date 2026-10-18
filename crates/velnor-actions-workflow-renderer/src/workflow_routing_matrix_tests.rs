use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

use super::super::{MAX_MATRIX_COMBINATIONS, WorkflowRoutingPolicy, route_workflow_yaml};
use crate::{RenderError, yaml::Yaml};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn scale_set() -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::from_mode(ExecutionMode::ScaleSet, selector())
}

fn workflow(job: Yaml) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![("build".to_owned(), job)]),
    )])
}

fn runner_job(expression: &str, matrix: Yaml) -> Yaml {
    Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str(expression)),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![("matrix".to_owned(), matrix)]),
        ),
    ])
}

fn assert_routed_runner(workflow: &Yaml) {
    let Yaml::Map(document) = workflow else {
        panic!("workflow should be a map");
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("workflow jobs should be a map");
    };
    let Some((_, Yaml::Map(fields))) = jobs.iter().find(|(key, _)| key == "build") else {
        panic!("build job should be a map");
    };
    assert_eq!(
        fields
            .iter()
            .find(|(key, _)| key == "runs-on")
            .map(|(_, value)| value),
        Some(&Yaml::Flow(selector().labels().to_vec()))
    );
}

#[test]
fn mixed_runner_axis_and_include_are_both_checked() {
    let matrix = Yaml::Map(vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04"), Yaml::str("macos-26")]),
        ),
        (
            "include".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::str("ubuntu-26.04"),
            )])]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("the base axis remains part of the finite runner choices");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:owned-tools.yml:build:macos-26".to_owned()
        )
    );
}

#[test]
fn unmatched_include_row_without_runner_fails_closed() {
    let matrix = Yaml::Map(vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04")]),
        ),
        ("os".to_owned(), Yaml::Seq(vec![Yaml::str("linux")])),
        (
            "include".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "os".to_owned(),
                Yaml::str("windows"),
            )])]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("GitHub adds the unmatched include as a row without runner");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "matrix_runner_value_missing:owned-tools.yml:build:${{ matrix.runner }}".to_owned()
        )
    );
}

#[test]
fn exclusions_remove_only_matching_combinations() {
    let matrix = Yaml::Map(vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04"), Yaml::str("macos-26")]),
        ),
        (
            "exclude".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::str("macos-26"),
            )])]),
        ),
    ]);
    let routed = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect("the remaining Linux axis value routes");
    assert_routed_runner(&routed);
}

#[test]
fn excluding_every_matrix_row_fails_closed() {
    let matrix = Yaml::Map(vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04")]),
        ),
        (
            "exclude".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::str("ubuntu-26.04"),
            )])]),
        ),
    ]);
    let error = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("no remaining row has a runner choice");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "matrix_runner_values_missing:checks.yml:build:${{ matrix.runner }}".to_owned()
        )
    );
}

#[test]
fn include_augments_surviving_finite_rows_after_exclusion() {
    let matrix = Yaml::Map(vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04"), Yaml::str("macos-26")]),
        ),
        ("os".to_owned(), Yaml::Seq(vec![Yaml::str("linux")])),
        (
            "exclude".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::str("macos-26"),
            )])]),
        ),
        (
            "include".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![
                ("runner".to_owned(), Yaml::str("ubuntu-26.04")),
                ("os".to_owned(), Yaml::str("linux")),
                ("tier".to_owned(), Yaml::str("extended")),
            ])]),
        ),
    ]);
    let routed = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect("the excluded Darwin row stays absent and the include augments Linux");
    assert_routed_runner(&routed);
}

#[test]
fn include_only_matrix_can_resolve_a_finite_object_property() {
    let matrix = Yaml::Map(vec![(
        "include".to_owned(),
        Yaml::Seq(vec![Yaml::Map(vec![(
            "runner".to_owned(),
            Yaml::Map(vec![("label".to_owned(), Yaml::str("ubuntu-26.04"))]),
        )])]),
    )]);
    let routed = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner.label }}", matrix)),
        &scale_set(),
    )
    .expect("finite include-only object property resolves");
    assert_routed_runner(&routed);
}

#[test]
fn unknown_and_missing_runner_expressions_fail_closed() {
    for (expression, matrix, expected) in [
        (
            "${{ vars.RUNNER }}",
            Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::Seq(vec![Yaml::str("ubuntu-26.04")]),
            )]),
            "unsupported_runs_on_for_linux_amd64_scale_set:checks.yml:build:${{ vars.RUNNER }}",
        ),
        (
            "${{ matrix.runner.label }}",
            Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::Seq(vec![Yaml::Map(vec![(
                    "name".to_owned(),
                    Yaml::str("ubuntu-26.04"),
                )])]),
            )]),
            "matrix_runner_value_missing:checks.yml:build:${{ matrix.runner.label }}",
        ),
    ] {
        let error = route_workflow_yaml(
            "checks.yml",
            workflow(runner_job(expression, matrix)),
            &scale_set(),
        )
        .expect_err("unbound runner choices cannot become Scale Set jobs");
        assert_eq!(error, RenderError::InvalidWorkflow(expected.to_owned()));
    }
}

#[test]
fn matrix_runner_expansion_has_a_finite_limit() {
    let runners = (0..=MAX_MATRIX_COMBINATIONS)
        .map(|_| Yaml::str("ubuntu-26.04"))
        .collect();
    let matrix = Yaml::Map(vec![("runner".to_owned(), Yaml::Seq(runners))]);
    let error = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("an overlarge finite matrix must be rejected");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow("matrix_too_large:checks.yml:build".to_owned())
    );
}

#[test]
fn matrix_axis_count_has_a_finite_limit() {
    let axes = (0..=MAX_MATRIX_COMBINATIONS)
        .map(|index| (format!("axis_{index}"), Yaml::Seq(vec![Yaml::str("one")])))
        .collect();
    let error = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", Yaml::Map(axes))),
        &scale_set(),
    )
    .expect_err("an overlarge matrix key set must be rejected before expansion");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow("matrix_too_large:checks.yml:build".to_owned())
    );
}
