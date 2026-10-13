//! Schema-2 workflow assembly: qualification, releases, monitoring.

use super::{
    GENERATOR_RELEASE_WORKFLOW, IMAGE_RELEASE_WORKFLOW, MACOS_BINARY_RELEASE_WORKFLOW,
    MONITORING_WORKFLOW, QUALIFICATION_WORKFLOW, RunnerSpec,
};
use super::{classes, features, mbx_qualification, release};
use velnor_actions_contract_config::RoutingWorkflow;
use velnor_actions_workflow_generator::Schema2WorkflowRequest;
use velnor_actions_workflow_generator::generator_release;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::marker::with_marker;
use velnor_actions_workflow_tree::rendered::RenderedFile;
use velnor_actions_workflow_tree::runs_on::runs_on_yaml;
use velnor_actions_workflow_tree::yaml::{Yaml, render_yaml};

/// Render every requested workflow. Empty when nothing is requested.
///
/// # Errors
///
/// Illegal selectors or a bad generator version fail.
pub fn render_schema2_workflows(
    request: &Schema2WorkflowRequest,
) -> Result<Vec<RenderedFile>, RenderError> {
    RunnerSpec::hosted(&request.hosted_label)?;
    let mut files = Vec::new();
    if request.workflows.contains(&RoutingWorkflow::Qualification) {
        files.push(file(
            QUALIFICATION_WORKFLOW,
            &request.version,
            &qualification(request)?,
        )?);
    }
    if request.workflows.contains(&RoutingWorkflow::ImageRelease) {
        files.push(file(
            IMAGE_RELEASE_WORKFLOW,
            &request.version,
            &release::image_release(request)?,
        )?);
    }
    if request
        .workflows
        .contains(&RoutingWorkflow::MacosBinaryRelease)
    {
        files.push(file(
            MACOS_BINARY_RELEASE_WORKFLOW,
            &request.version,
            &release::macos_binary_release(request)?,
        )?);
    }
    if request
        .workflows
        .contains(&RoutingWorkflow::GeneratorRelease)
    {
        let generated = generator_release::generator_release(request)?;
        files.push(file(
            GENERATOR_RELEASE_WORKFLOW,
            &request.version,
            &generated.workflow,
        )?);
        for (path, action) in generated.actions {
            files.push(file(&path, &request.version, &action)?);
        }
    }
    if request.workflows.contains(&RoutingWorkflow::Monitoring) {
        files.push(file(
            MONITORING_WORKFLOW,
            &request.version,
            &monitoring(request)?,
        )?);
    }
    Ok(files)
}

fn file(path: &str, version: &str, body: &Yaml) -> Result<RenderedFile, RenderError> {
    let bytes = with_marker(version, &render_yaml(body))?;
    velnor_actions_workflow_tree::workflow_size::check_workflow_size(path, &bytes)?;
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes,
    })
}

pub(crate) fn qualification(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = RunnerSpec::hosted(&request.hosted_label)?;
    let scale = RunnerSpec::scale_set(runs_on_yaml(&request.scale_set.token())?);
    let echo = "inputs.mode == 'both'";
    let mut jobs = vec![
        with_if(
            job(
                "verify-hosted",
                "Verify / GitHub hosted / Linux x64",
                &hosted,
                30,
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
                30,
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
                10,
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
    Ok(document("Qualification", mode_trigger(), jobs))
}

pub(crate) fn with_if((id, body): (String, Yaml), when: &str) -> (String, Yaml) {
    let Yaml::Map(mut fields) = body else {
        return (id, body);
    };
    fields.insert(1, ("if".to_owned(), Yaml::str(when)));
    (id, Yaml::Map(fields))
}

pub(crate) fn monitoring(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
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
                30,
                Vec::new(),
                "Run scale-set lane",
                "echo scale-set-lane",
            ),
            job(
                "queue-monitor",
                "Queue monitor",
                &hosted,
                10,
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
    timeout: i64,
    needs: Vec<String>,
    step_name: &str,
    run: &str,
) -> (String, Yaml) {
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runner.runs_on.clone()),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
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
