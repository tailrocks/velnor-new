//! Qualification, image-release, macOS-binary-release, generator-release,
//! and monitoring workflows. Emitted only when schema 2 requests them.

use std::collections::BTreeSet;

use velnor_actions_contract::{RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::runs_on::runs_on_yaml;
use crate::setup::MiseSetup;
use crate::yaml::{Yaml, render_yaml};

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
const QUALIFICATION_RUN_NAME: &str = "${{ (inputs.mode == 'mbx-cancel-pre-save-controller' || inputs.mode == 'mbx-cancel-pre-save-victim' || inputs.mode == 'mbx-cancel-during-save-controller' || inputs.mode == 'mbx-cancel-during-save-victim') && format('MBX cancellation {0} {1}', inputs.mode, inputs.probe_id) || '' }}";
/// Image-release workflow path.
pub const IMAGE_RELEASE_WORKFLOW: &str = ".github/workflows/image-release.yml";
/// macOS binary-release workflow path.
pub const MACOS_BINARY_RELEASE_WORKFLOW: &str = ".github/workflows/macos-binary-release.yml";
/// Generator-release workflow path.
pub const GENERATOR_RELEASE_WORKFLOW: &str = ".github/workflows/generator-release.yml";
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";

#[path = "schema2_classes.rs"]
mod classes;
#[path = "schema2_features.rs"]
mod features;
#[path = "schema2_generator_release.rs"]
mod generator_release;
#[path = "schema2_mbx_cancel_probe.rs"]
mod mbx_cancel_probe;
#[path = "schema2_mbx_corrupt_probe.rs"]
mod mbx_corrupt_probe;
#[path = "schema2_mbx_parallel_probe.rs"]
mod mbx_parallel_probe;
#[path = "schema2_mbx_qualification.rs"]
mod mbx_qualification;
#[path = "schema2_mbx_qualification_helpers.rs"]
mod mbx_qualification_helpers;
#[path = "schema2_mbx_resource_probe.rs"]
mod mbx_resource_probe;
#[path = "schema2_mbx_resource_probe_env.rs"]
mod mbx_resource_probe_env;
#[path = "schema2_mbx_resource_probe_render.rs"]
mod mbx_resource_probe_render;
#[path = "schema2_mbx_roundtrip_terminal.rs"]
mod mbx_roundtrip_terminal;
#[path = "schema2_mbx_stock_restore.rs"]
mod mbx_stock_restore;
#[path = "schema2_release.rs"]
mod release;

/// Which schema 2 workflows to emit, plus the selectors they use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema2WorkflowRequest {
    /// Generator version for the marker.
    pub version: String,
    /// Hosted catalog label.
    pub hosted_label: String,
    /// Validated scale-set selector.
    pub scale_set: ScaleSetSelector,
    /// Workflows to emit. Empty emits nothing.
    pub workflows: BTreeSet<RoutingWorkflow>,
    /// Pinned tool inputs, required only when qualification is emitted.
    pub mbx_qualification: Option<MbxQualificationPins>,
}

/// Exact tools used by the hosted MBX cache qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbxQualificationPins {
    /// Resolved Mise action and binary pins.
    pub mise_setup: MiseSetup,
    /// Full-SHA MBX GitHub Action ref.
    pub mbx_action_uses: String,
    /// Exact MBX tool version.
    pub mbx_version: String,
    /// Exact Rust toolchain version used by the qualification lane.
    pub rust_version: String,
}

impl Schema2WorkflowRequest {
    /// Canonical scale-set selector (`velnor`, then the scale-set name).
    ///
    /// # Errors
    ///
    /// Illegal labels fail.
    pub fn canonical_scale_set() -> Result<ScaleSetSelector, RenderError> {
        ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))
    }
}

/// Render every requested workflow. Empty when nothing is requested.
///
/// # Errors
///
/// Illegal selectors or a bad generator version fail.
pub fn render_schema2_workflows(
    request: &Schema2WorkflowRequest,
) -> Result<Vec<RenderedFile>, RenderError> {
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
        files.push(file(
            GENERATOR_RELEASE_WORKFLOW,
            &request.version,
            &generator_release::generator_release(request)?,
        )?);
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
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes: with_marker(version, &render_yaml(body))?,
    })
}

fn qualification(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let scale = runs_on_yaml(&request.scale_set.token())?;
    let echo = "inputs.mode == 'both'";
    let mut jobs = vec![
        with_if(
            job(
                "verify-hosted",
                "Verify / GitHub hosted / Linux x64",
                hosted.clone(),
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
                scale.clone(),
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
                hosted.clone(),
                10,
                vec!["verify-hosted".to_owned(), "verify-scale-set".to_owned()],
                "Compare lanes",
                "echo compare-lanes",
            ),
            echo,
        ),
    ];
    jobs.extend(features::feature_jobs(hosted.clone(), scale.clone()));
    jobs.extend(features::negative_jobs(hosted.clone(), scale.clone()));
    jobs.extend(classes::class_jobs(&hosted, &scale));
    if let Some(pins) = &request.mbx_qualification {
        jobs.extend(mbx_qualification::jobs(pins, &hosted)?);
    } else if request.workflows.contains(&RoutingWorkflow::Qualification) {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_qualification_pins".to_owned(),
        ));
    }
    let mut workflow = document("Qualification", mode_trigger(), jobs);
    if let Yaml::Map(fields) = &mut workflow {
        fields.insert(
            1,
            ("run-name".to_owned(), Yaml::str(QUALIFICATION_RUN_NAME)),
        );
    }
    Ok(workflow)
}

fn with_if((id, body): (String, Yaml), when: &str) -> (String, Yaml) {
    let Yaml::Map(mut fields) = body else {
        return (id, body);
    };
    fields.insert(1, ("if".to_owned(), Yaml::str(when)));
    (id, Yaml::Map(fields))
}

fn monitoring(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    Ok(document(
        "Scale set monitoring",
        empty_dispatch(),
        vec![
            job(
                "scale-set-lane",
                "Scale set lane",
                runs_on_yaml(&request.scale_set.token())?,
                30,
                Vec::new(),
                "Run scale-set lane",
                "echo scale-set-lane",
            ),
            job(
                "queue-monitor",
                "Queue monitor",
                runs_on_yaml(&request.hosted_label)?,
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
            Yaml::Map(vec![
                (
                    "mode".to_owned(),
                    Yaml::Map(vec![
                        ("type".to_owned(), Yaml::str("string")),
                        ("required".to_owned(), Yaml::Bool(false)),
                        ("default".to_owned(), Yaml::str("both")),
                    ]),
                ),
                (
                    "probe_id".to_owned(),
                    Yaml::Map(vec![
                        ("type".to_owned(), Yaml::str("string")),
                        ("required".to_owned(), Yaml::Bool(false)),
                        ("default".to_owned(), Yaml::str("")),
                    ]),
                ),
            ]),
        )]),
    )])
}

fn empty_dispatch() -> Yaml {
    Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))])
}

fn job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    timeout: i64,
    needs: Vec<String>,
    step_name: &str,
    run: &str,
) -> (String, Yaml) {
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
    ];
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
