use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

use super::{WorkflowRoutingPolicy, route_workflow_yaml, validate_generated_workflow_calls};
use crate::{RenderError, RenderedFile, yaml::Yaml};

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

fn job(runs_on: Yaml) -> Yaml {
    Yaml::Map(vec![("runs-on".to_owned(), runs_on)])
}

fn workflow(jobs: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(
            jobs.into_iter()
                .map(|(id, job)| (id.to_owned(), job))
                .collect(),
        ),
    )])
}

#[test]
fn hosted_and_both_preserve_existing_placement() {
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        let policy = WorkflowRoutingPolicy::from_mode(mode, selector());
        let input = workflow(vec![("build", job(Yaml::str("ubuntu-26.04")))]);
        assert_eq!(
            route_workflow_yaml("ci.yml", input.clone(), &policy),
            Ok(input)
        );
    }
}

#[test]
fn linux_amd64_runner_is_routed_to_the_validated_selector() {
    let input = workflow(vec![("build", job(Yaml::str("ubuntu-24.04")))]);
    let expected = workflow(vec![(
        "build",
        job(Yaml::Flow(selector().labels().to_vec())),
    )]);
    assert_eq!(
        route_workflow_yaml("release.yml", input, &scale_set()),
        Ok(expected)
    );
}

#[test]
fn matrix_runner_values_are_resolved_and_darwin_is_explicit() {
    let runner_job = Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("${{ matrix.runner }}")),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![(
                "matrix".to_owned(),
                Yaml::Map(vec![(
                    "include".to_owned(),
                    Yaml::Seq(vec![
                        Yaml::Map(vec![("runner".to_owned(), Yaml::str("ubuntu-26.04"))]),
                        Yaml::Map(vec![("runner".to_owned(), Yaml::str("ubuntu-24.04-arm"))]),
                        Yaml::Map(vec![("runner".to_owned(), Yaml::str("macos-26"))]),
                    ]),
                )]),
            )]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(vec![("build", runner_job)]),
        &scale_set(),
    )
    .expect_err("native matrix must reject Darwin");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:owned-tools.yml:build:macos-26".to_owned()
        )
    );
}

#[test]
fn matrix_runner_axis_remains_in_scope_when_include_is_present() {
    let runner_job = Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("${{ matrix.runner }}")),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![(
                "matrix".to_owned(),
                Yaml::Map(vec![
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
                ]),
            )]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(vec![("build", runner_job)]),
        &scale_set(),
    )
    .expect_err("matrix axis must be checked alongside included values");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:owned-tools.yml:build:macos-26".to_owned()
        )
    );
}

#[test]
fn matrix_include_runner_must_be_explicit_even_with_base_axes() {
    let runner_job = Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("${{ matrix.runner }}")),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![(
                "matrix".to_owned(),
                Yaml::Map(vec![
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
                ]),
            )]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(vec![("build", runner_job)]),
        &scale_set(),
    )
    .expect_err("an unmatched include row must resolve the runner expression");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "matrix_runner_value_missing:owned-tools.yml:build:runner".to_owned()
        )
    );
}

#[test]
fn matrix_runner_exclusions_fail_closed() {
    let runner_job = Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("${{ matrix.runner }}")),
        (
            "strategy".to_owned(),
            Yaml::Map(vec![(
                "matrix".to_owned(),
                Yaml::Map(vec![
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
                ]),
            )]),
        ),
    ]);
    let error = route_workflow_yaml(
        "owned-tools.yml",
        workflow(vec![("build", runner_job)]),
        &scale_set(),
    )
    .expect_err("excluded matrix choices require expansion before routing");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "matrix_runner_exclusions_unsupported:owned-tools.yml:build:runner".to_owned()
        )
    );
}

#[test]
fn external_reusable_jobs_fail_closed() {
    let workflow = workflow(vec![(
        "delegate",
        Yaml::Map(vec![(
            "uses".to_owned(),
            Yaml::str("owner/repository/.github/workflows/check.yml@0123456789abcdef"),
        )]),
    )]);
    let error = route_workflow_yaml("ci.yml", workflow, &scale_set())
        .expect_err("external workflow has unresolved placement");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_external_reusable_workflow:ci.yml:delegate:owner/repository/.github/workflows/check.yml@0123456789abcdef".to_owned()
        )
    );
}

#[test]
fn local_reusable_jobs_resolve_after_all_workflow_files_are_collected() {
    let policy = scale_set();
    let caller = RenderedFile {
        path: ".github/workflows/product-release.yml".to_owned(),
        bytes: "# generated\njobs:\n  images:\n    uses: ./.github/workflows/product-release-images.yml\n".to_owned(),
    };
    let target = RenderedFile {
        path: ".github/workflows/product-release-images.yml".to_owned(),
        bytes: "# generated\njobs: {}\n".to_owned(),
    };
    let mut inventory = vec![caller.clone(), target];
    for path in [
        ".github/workflows/release.yml",
        ".github/workflows/binary-release.yml",
        ".github/workflows/tofu-apply.yml",
        ".github/workflows/suites.yml",
    ] {
        inventory.push(RenderedFile {
            path: path.to_owned(),
            bytes: "# generated\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n".to_owned(),
        });
    }
    assert_eq!(
        validate_generated_workflow_calls(&inventory, &policy),
        Ok(())
    );
    inventory.retain(|file| file.path != ".github/workflows/product-release-images.yml");
    let error = validate_generated_workflow_calls(&inventory, &policy)
        .expect_err("missing final generated target must fail");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unresolved_local_reusable_workflow:.github/workflows/product-release.yml:./.github/workflows/product-release-images.yml".to_owned()
        )
    );
}
