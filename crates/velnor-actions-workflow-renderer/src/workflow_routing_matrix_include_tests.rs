use super::super::super::{route_workflow_yaml, validate_generated_workflow_calls};
use super::super::expand_matrix;
use super::{runner_job, scale_set, selector, workflow};
use crate::{RenderError, RenderedFile, yaml::Yaml};

fn fixed_scale_set_job(matrix: Yaml) -> Yaml {
    Yaml::Map(vec![
        (
            "runs-on".to_owned(),
            Yaml::Flow(selector().labels().to_vec()),
        ),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![("matrix".to_owned(), matrix)]),
        ),
    ])
}

#[test]
fn include_can_add_a_previously_excluded_row() {
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
        (
            "include".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![(
                "runner".to_owned(),
                Yaml::str("macos-26"),
            )])]),
        ),
    ]);
    let error = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("include entries run after exclusions and can add rows back");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:checks.yml:build:macos-26".to_owned()
        )
    );
}

#[test]
fn later_include_overwrites_added_metadata_only_on_original_rows() {
    let matrix = vec![
        (
            "runner".to_owned(),
            Yaml::Seq(vec![Yaml::str("ubuntu-26.04"), Yaml::str("macos-26")]),
        ),
        (
            "include".to_owned(),
            Yaml::Seq(vec![
                Yaml::Map(vec![("tier".to_owned(), Yaml::str("general"))]),
                Yaml::Map(vec![
                    ("runner".to_owned(), Yaml::str("ubuntu-26.04")),
                    ("tier".to_owned(), Yaml::str("linux")),
                ]),
            ]),
        ),
    ];
    let rows =
        expand_matrix(&matrix, "checks.yml", "build").expect("finite include entries expand");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("tier"), Some(&Yaml::str("linux")));
    assert_eq!(rows[1].get("tier"), Some(&Yaml::str("general")));
}

#[test]
fn independent_appended_darwin_include_remains_a_separate_job() {
    let matrix = Yaml::Map(vec![
        ("fruit".to_owned(), Yaml::Seq(vec![Yaml::str("apple")])),
        (
            "include".to_owned(),
            Yaml::Seq(vec![
                Yaml::Map(vec![("runner".to_owned(), Yaml::str("ubuntu-26.04"))]),
                Yaml::Map(vec![
                    ("fruit".to_owned(), Yaml::str("banana")),
                    ("runner".to_owned(), Yaml::str("macos-26")),
                ]),
                Yaml::Map(vec![
                    ("fruit".to_owned(), Yaml::str("banana")),
                    ("runner".to_owned(), Yaml::str("ubuntu-26.04")),
                ]),
            ]),
        ),
    ]);
    let error = route_workflow_yaml(
        "checks.yml",
        workflow(runner_job("${{ matrix.runner }}", matrix)),
        &scale_set(),
    )
    .expect_err("the original matrix includes a Darwin runner");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:checks.yml:build:macos-26".to_owned()
        )
    );
}

#[test]
fn final_byte_gate_cannot_recover_runner_axis_after_rewrite() {
    let matrix = Yaml::Map(vec![
        ("fruit".to_owned(), Yaml::Seq(vec![Yaml::str("apple")])),
        (
            "include".to_owned(),
            Yaml::Seq(vec![
                Yaml::Map(vec![("runner".to_owned(), Yaml::str("ubuntu-26.04"))]),
                Yaml::Map(vec![
                    ("fruit".to_owned(), Yaml::str("banana")),
                    ("runner".to_owned(), Yaml::str("macos-26")),
                ]),
                Yaml::Map(vec![
                    ("fruit".to_owned(), Yaml::str("banana")),
                    ("runner".to_owned(), Yaml::str("ubuntu-26.04")),
                ]),
            ]),
        ),
    ]);
    let file = RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: crate::yaml::render_yaml(&workflow(fixed_scale_set_job(matrix))),
    };
    let error = validate_generated_workflow_calls(&[file], &scale_set())
        .expect_err("final workflow inventory requires the selector container");
    assert!(matches!(error, RenderError::InvalidWorkflow(_)));
}
