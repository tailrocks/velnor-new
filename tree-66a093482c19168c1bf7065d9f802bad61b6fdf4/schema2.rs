//! Qualification, image-release, macOS-binary-release, and monitoring
//! workflows. Emitted only when schema 2 requests them.

use std::collections::BTreeSet;

use velnor_actions_contract::{RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::runs_on::runs_on_yaml;
use crate::yaml::{Yaml, render_yaml};

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
/// Image-release workflow path.
pub const IMAGE_RELEASE_WORKFLOW: &str = ".github/workflows/image-release.yml";
/// macOS binary-release workflow path.
pub const MACOS_BINARY_RELEASE_WORKFLOW: &str = ".github/workflows/macos-binary-release.yml";
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";
/// JavaScript, service, artifact, and Buildx qualification path.
pub const QUALIFICATION_FEATURES_WORKFLOW: &str = ".github/workflows/qualification-features.yml";
/// Intentionally failing qualification path. Not part of the green suite.
pub const QUALIFICATION_NEGATIVE_WORKFLOW: &str = ".github/workflows/qualification-negative.yml";

#[path = "schema2_features.rs"]
mod features;

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
        files.push(file(
            QUALIFICATION_FEATURES_WORKFLOW,
            &request.version,
            &features::features(request)?,
        )?);
        files.push(file(
            QUALIFICATION_NEGATIVE_WORKFLOW,
            &request.version,
            &features::negative(request)?,
        )?);
    }
    if request.workflows.contains(&RoutingWorkflow::ImageRelease) {
        files.push(file(
            IMAGE_RELEASE_WORKFLOW,
            &request.version,
            &single(request, "Image release", "image-release", "Publish image")?,
        )?);
    }
    if request
        .workflows
        .contains(&RoutingWorkflow::MacosBinaryRelease)
    {
        files.push(file(
            MACOS_BINARY_RELEASE_WORKFLOW,
            &request.version,
            &single(
                request,
                "macOS binary release",
                "macos-binary-release",
                "Publish macOS binary",
            )?,
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
    Ok(document(
        "Qualification",
        mode_trigger(),
        vec![
            job(
                "verify-hosted",
                "Verify / GitHub hosted / Linux x64",
                hosted,
                30,
                Vec::new(),
                "Qualify hosted lane",
                "echo qualification-hosted",
            ),
            job(
                "verify-scale-set",
                "Verify / Velnor Scale Set / Linux x64",
                scale,
                30,
                Vec::new(),
                "Qualify scale-set lane",
                "echo qualification-scale-set",
            ),
            job(
                "compare",
                "Compare hosted and Velnor execution",
                runs_on_yaml(&request.hosted_label)?,
                10,
                vec!["verify-hosted".to_owned(), "verify-scale-set".to_owned()],
                "Compare lanes",
                "echo compare-lanes",
            ),
        ],
    ))
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

fn single(
    request: &Schema2WorkflowRequest,
    title: &str,
    id: &str,
    step: &str,
) -> Result<Yaml, RenderError> {
    Ok(document(
        title,
        empty_dispatch(),
        vec![job(
            id,
            title,
            runs_on_yaml(&request.hosted_label)?,
            10,
            Vec::new(),
            step,
            "echo release",
        )],
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
