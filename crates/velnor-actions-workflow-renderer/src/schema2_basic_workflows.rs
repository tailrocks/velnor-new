use velnor_actions_contract::{JobTimeout, RoutingWorkflow};

use super::{
    RunnerSpec, Schema2WorkflowRequest, classes, features, mbx_qualification,
    mise_pin_qualification,
};
use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;

pub(super) fn qualification(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    if request.routing.is_scale_set() {
        return Ok(verification_only_qualification());
    }
    let hosted = RunnerSpec::hosted(&request.hosted_label)?;
    let scale = RunnerSpec::scale_set(runs_on_yaml(&request.scale_set.token())?);
    let echo = "inputs.mode == 'both'";
    let mut jobs = vec![
        with_if(
            job(
                "verify-hosted",
                "Verify / GitHub hosted / Linux x64",
                &hosted,
                JobTimeout::DEFAULT,
                Vec::new(),
                "Qualify hosted lane",
                "echo qualification-hosted",
            ),
            echo,
        ),
        with_if(
            job(
                "verify-scale-set",
                "Verify / Velnor Scale Set / Linux x64",
                &scale,
                JobTimeout::DEFAULT,
                Vec::new(),
                "Qualify scale-set lane",
                "echo qualification-scale-set",
            ),
            echo,
        ),
        with_if(
            job(
                "compare",
                "Compare hosted and Velnor execution",
                &hosted,
                JobTimeout::DEFAULT,
                vec!["verify-hosted".to_owned(), "verify-scale-set".to_owned()],
                "Compare lanes",
                "echo compare-lanes",
            ),
            echo,
        ),
    ];
    jobs.extend(features::feature_jobs(&hosted, &scale));
    jobs.extend(features::negative_jobs(&hosted, &scale));
    jobs.extend(classes::class_jobs(&hosted, &scale));
    if let Some(pins) = &request.mbx_qualification {
        jobs.extend(mbx_qualification::jobs(pins, &hosted)?);
    } else if request.workflows.contains(&RoutingWorkflow::Qualification) {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_qualification_pins".to_owned(),
        ));
    }
    if let Some(pins) = &request.mise_pin_qualification {
        jobs.extend(mise_pin_qualification::jobs(pins)?);
    } else if request.workflows.contains(&RoutingWorkflow::Qualification) {
        return Err(RenderError::InvalidWorkflow(
            "missing_mise_pin_qualification_pins".to_owned(),
        ));
    }
    Ok(document("Qualification", mode_trigger(), jobs))
}

fn verification_only_qualification() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Qualification")),
        ("on".to_owned(), verification_dispatch_trigger()),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("read")),
                ("contents".to_owned(), Yaml::str("read")),
            ]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("velnor-verification-wrapper-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "verify-full-ci".to_owned(),
                Yaml::Map(vec![
                    (
                        "name".to_owned(),
                        Yaml::str("Run CI verification on the Velnor Scale Set"),
                    ),
                    (
                        "if".to_owned(),
                        Yaml::str("inputs.mode == 'verification-only'"),
                    ),
                    ("uses".to_owned(), Yaml::str("./.github/workflows/ci.yml")),
                ]),
            )]),
        ),
    ])
}

fn verification_dispatch_trigger() -> Yaml {
    Yaml::Map(vec![(
        "workflow_dispatch".to_owned(),
        Yaml::Map(vec![("inputs".to_owned(), verification_dispatch_inputs())]),
    )])
}

fn verification_dispatch_inputs() -> Yaml {
    Yaml::Map(vec![(
        "mode".to_owned(),
        Yaml::Map(vec![
            ("type".to_owned(), Yaml::str("choice")),
            ("required".to_owned(), Yaml::Bool(true)),
            (
                "options".to_owned(),
                Yaml::Seq(vec![Yaml::str("verification-only")]),
            ),
        ]),
    )])
}

pub(crate) fn with_if((id, body): (String, Yaml), when: &str) -> (String, Yaml) {
    let Yaml::Map(mut fields) = body else {
        return (id, body);
    };
    fields.insert(1, ("if".to_owned(), Yaml::str(when)));
    (id, Yaml::Map(fields))
}

pub(super) fn monitoring(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let scale = RunnerSpec::scale_set(runs_on_yaml(&request.scale_set.token())?);
    let hosted = RunnerSpec::hosted(&request.hosted_label)?;
    Ok(document(
        "Scale set monitoring",
        empty_dispatch(),
        vec![
            job(
                "scale-set-lane",
                "Scale set lane",
                &scale,
                JobTimeout::DEFAULT,
                Vec::new(),
                "Run scale-set lane",
                "echo scale-set-lane",
            ),
            job(
                "queue-monitor",
                "Queue monitor",
                &hosted,
                JobTimeout::DEFAULT,
                Vec::new(),
                "Watch admission",
                "echo queue-monitor",
            ),
        ],
    ))
}

fn document(name: &str, on: Yaml, jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("on".to_owned(), on),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

fn mode_trigger() -> Yaml {
    Yaml::Map(vec![(
        "workflow_dispatch".to_owned(),
        Yaml::Map(vec![(
            "inputs".to_owned(),
            Yaml::Map(vec![(
                "mode".to_owned(),
                Yaml::Map(vec![
                    ("type".to_owned(), Yaml::str("string")),
                    ("required".to_owned(), Yaml::Bool(false)),
                    ("default".to_owned(), Yaml::str("both")),
                ]),
            )]),
        )]),
    )])
}

fn empty_dispatch() -> Yaml {
    Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))])
}

fn job(
    id: &str,
    name: &str,
    runner: &RunnerSpec,
    timeout: JobTimeout,
    needs: Vec<String>,
    step_name: &str,
    run: &str,
) -> (String, Yaml) {
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runner.runs_on.clone()),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(timeout.minutes())),
        ),
    ];
    runner.push_default_shell(&mut fields, false);
    if !needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(needs.into_iter().map(Yaml::str).collect()),
        ));
    }
    fields.push((
        "steps".to_owned(),
        Yaml::Seq(vec![Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(step_name)),
            ("run".to_owned(), Yaml::str(run)),
        ])]),
    ));
    (id.to_owned(), Yaml::Map(fields))
}
