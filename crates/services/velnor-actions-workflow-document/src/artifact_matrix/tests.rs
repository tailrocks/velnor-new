use super::{
    ArtifactMatrixDirective, artifact_matrix_directives, attach_artifact_matrices,
    scrub_artifact_matrix_markers,
};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract_workflow::{
    ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV, ARTIFACT_MATRIX_PROVIDER_ENV,
    ArtifactBuildProvider, Job, JobTimeout, Step, StepKind,
};
use velnor_actions_workflow_tree::yaml::Yaml;

fn matrix_job(runs_on: &str) -> Job {
    Job {
        outputs: Vec::new(),
        display_name: "Artifact builds".to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "Run matrix task".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["mise".to_owned(), "run".to_owned(), "$TASK".to_owned()],
                env: BTreeMap::from([
                    (ARTIFACT_MATRIX_NEEDS_JOB_ENV.to_owned(), "plan".to_owned()),
                    (
                        ARTIFACT_MATRIX_PROVIDER_ENV.to_owned(),
                        "provider".to_owned(),
                    ),
                    (ARTIFACT_MATRIX_MAX_PARALLEL_ENV.to_owned(), "3".to_owned()),
                ]),
            },
        }],
    }
}

fn document() -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                "plan".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("id".to_owned(), Yaml::str("plan")),
                        ("env".to_owned(), Yaml::Map(Vec::new())),
                    ])]),
                )]),
            ),
            (
                "artifact-build__hosted".to_owned(),
                Yaml::Map(vec![
                    ("name".to_owned(), Yaml::str("Build artifact")),
                    ("timeout-minutes".to_owned(), Yaml::Int(360)),
                    ("steps".to_owned(), Yaml::Seq(Vec::new())),
                ]),
            ),
            (
                "artifact-build__local".to_owned(),
                Yaml::Map(vec![
                    ("name".to_owned(), Yaml::str("Build artifact")),
                    ("timeout-minutes".to_owned(), Yaml::Int(360)),
                    ("steps".to_owned(), Yaml::Seq(Vec::new())),
                ]),
            ),
        ]),
    )])
}

fn map_value<'a>(entries: &'a [(String, Yaml)], key: &str) -> &'a Yaml {
    entries
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .expect("key exists")
}

#[test]
fn provider_directives_come_from_expanded_runner_identity() {
    let jobs = BTreeMap::from([
        (
            "artifact-build__hosted".to_owned(),
            matrix_job("ubuntu-26.04"),
        ),
        (
            "artifact-build__local".to_owned(),
            matrix_job("scale-set:velnor+ubuntu-26.04-scale-set"),
        ),
    ]);
    let directives = artifact_matrix_directives(&jobs).expect("valid directives");
    assert_eq!(
        directives,
        vec![
            ArtifactMatrixDirective {
                job_id: "artifact-build__hosted".to_owned(),
                provider: ArtifactBuildProvider::GithubHosted,
                max_parallel: 3,
            },
            ArtifactMatrixDirective {
                job_id: "artifact-build__local".to_owned(),
                provider: ArtifactBuildProvider::VelnorScaleSet,
                max_parallel: 3,
            },
        ]
    );
}

#[test]
fn provider_matrices_are_attached_with_unique_plan_outputs_and_no_fail_fast() {
    let directives = vec![
        ArtifactMatrixDirective {
            job_id: "artifact-build__hosted".to_owned(),
            provider: ArtifactBuildProvider::GithubHosted,
            max_parallel: 3,
        },
        ArtifactMatrixDirective {
            job_id: "artifact-build__local".to_owned(),
            provider: ArtifactBuildProvider::VelnorScaleSet,
            max_parallel: 3,
        },
    ];
    let mut rendered = document();
    attach_artifact_matrices(&mut rendered, &directives).expect("attach matrices");
    let Yaml::Map(root) = rendered else {
        panic!("workflow map")
    };
    let Yaml::Map(jobs) = map_value(&root, "jobs") else {
        panic!("jobs map")
    };
    let Yaml::Map(plan) = map_value(jobs, "plan") else {
        panic!("plan map")
    };
    let Yaml::Map(outputs) = map_value(plan, "outputs") else {
        panic!("plan outputs")
    };
    let output_names = outputs
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        output_names,
        BTreeSet::from(["artifact_hosted_matrix", "artifact_velnor_matrix"])
    );
    let Yaml::Seq(plan_steps) = map_value(plan, "steps") else {
        panic!("plan steps")
    };
    let Yaml::Map(plan_step) = &plan_steps[0] else {
        panic!("plan step")
    };
    let Yaml::Map(plan_env) = map_value(plan_step, "env") else {
        panic!("plan env")
    };
    assert!(plan_env.iter().any(|(name, value)| {
        name == "VELNOR_PLAN_MATRIX_OUTPUT_MODE" && value == &Yaml::str("dynamic_matrix")
    }));

    for (job_id, expected_output) in [
        ("artifact-build__hosted", "artifact_hosted_matrix"),
        ("artifact-build__local", "artifact_velnor_matrix"),
    ] {
        let Yaml::Map(job) = map_value(jobs, job_id) else {
            panic!("artifact job")
        };
        assert_eq!(
            map_value(job, "name"),
            &Yaml::str(
                "${{ format('Build artifact {0} / {1}', matrix.provider, matrix.task_id) }}"
            )
        );
        let Yaml::Map(strategy) = map_value(job, "strategy") else {
            panic!("strategy")
        };
        assert_eq!(map_value(strategy, "fail-fast"), &Yaml::Bool(false));
        assert_eq!(map_value(strategy, "max-parallel"), &Yaml::Int(3));
        assert_eq!(
            map_value(job, "timeout-minutes"),
            &Yaml::str("${{ matrix.timeout_minutes }}")
        );
        assert_eq!(
            map_value(strategy, "matrix"),
            &Yaml::str(format!(
                "${{{{ fromJSON(needs.plan.outputs.{expected_output}) }}}}"
            ))
        );
    }
}

#[test]
fn artifact_matrix_markers_are_removed_before_step_rendering() {
    let jobs = BTreeMap::from([(
        "artifact-build__hosted".to_owned(),
        matrix_job("ubuntu-26.04"),
    )]);
    let scrubbed = scrub_artifact_matrix_markers(&jobs);
    let step = scrubbed["artifact-build__hosted"]
        .steps
        .first()
        .expect("step");
    let StepKind::Shell { env, .. } = &step.kind else {
        panic!("shell step")
    };
    assert!(env.is_empty());
}

#[test]
fn malformed_or_ambiguous_artifact_matrix_markers_fail_closed() {
    let mut partial = matrix_job("ubuntu-26.04");
    let StepKind::Shell { env, .. } = &mut partial.steps[0].kind else {
        unreachable!()
    };
    env.remove(ARTIFACT_MATRIX_PROVIDER_ENV);
    let jobs = BTreeMap::from([("artifact-build__hosted".to_owned(), partial)]);
    assert!(
        artifact_matrix_directives(&jobs)
            .expect_err("partial marker")
            .to_string()
            .contains("artifact_matrix_marker_partial")
    );

    let jobs = BTreeMap::from([
        (
            "artifact-build__hosted".to_owned(),
            matrix_job("ubuntu-26.04"),
        ),
        (
            "artifact-build__other".to_owned(),
            matrix_job("ubuntu-26.04"),
        ),
    ]);
    assert!(
        artifact_matrix_directives(&jobs)
            .expect_err("duplicate provider")
            .to_string()
            .contains("artifact_matrix_duplicate_provider_job")
    );
}
