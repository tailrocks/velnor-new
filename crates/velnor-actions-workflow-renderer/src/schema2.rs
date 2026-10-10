//! Qualification, composed product-release, and monitoring workflows.
//! Emitted only when schema 2 requests them.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ReleaseTarget, RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL,
};

use crate::RenderError;
use crate::render::RenderedFile;
use crate::runs_on::runs_on_yaml;
use crate::setup::MiseSetup;
use crate::yaml::Yaml;

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
/// Shared product-release workflow path.
pub const PRODUCT_RELEASE_WORKFLOW: &str = product_release::WORKFLOW_PATH;
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";

#[path = "schema2_classes.rs"]
mod classes;
#[path = "schema2_runner_spec.rs"]
mod runner_spec;
use runner_spec::RunnerSpec;
#[path = "schema2_consumer_binary_release.rs"]
mod consumer_binary_release;
#[path = "schema2_consumer_binary_release_scripts.rs"]
mod consumer_binary_release_scripts;
#[path = "schema2_consumer_release_eligibility.rs"]
mod consumer_release_eligibility;
#[path = "schema2_features.rs"]
mod features;
#[path = "schema2_generator_release.rs"]
mod generator_release;
#[path = "schema2_mbx_qualification.rs"]
mod mbx_qualification;
#[path = "schema2_mise_pin_qualification.rs"]
mod mise_pin_qualification;
#[path = "schema2_product_release.rs"]
mod product_release;
#[path = "schema2_product_release_family.rs"]
mod product_release_family;
#[path = "schema2_product_release_pins.rs"]
mod product_release_pins;
#[path = "schema2_release.rs"]
mod release;
/// Exact-source gates for composed product-release workflows.
#[path = "schema2_release_eligibility.rs"]
pub mod release_eligibility;
#[path = "schema2_resource_probe_image.rs"]
mod resource_probe_image;
pub use product_release_pins::ProductReleasePins;

/// Render the generic one-package consumer binary release workflow.
///
/// # Errors
///
/// Returns render errors for invalid identities, pins, or commands.
pub fn render_consumer_binary_release(
    spec: &consumer_binary_release::ConsumerBinaryReleaseSpec,
) -> Result<RenderedFile, RenderError> {
    consumer_binary_release::render_consumer_binary_release(spec)
}

pub use consumer_binary_release::ConsumerBinaryReleaseSpec;

#[cfg(test)]
#[path = "schema2_runner_shell_tests.rs"]
mod runner_shell_tests;

#[cfg(test)]
#[path = "schema2_resource_probe_image_tests.rs"]
mod resource_probe_image_tests;

#[cfg(test)]
#[path = "schema2_product_release_test_pins.rs"]
pub(super) mod product_release_test_pins;

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
    /// Latest Mise release inputs, required only when qualification is emitted.
    pub mise_pin_qualification: Option<MisePinQualificationPins>,
    /// Orchestrator-resolved Mise setup and command vectors for product releases.
    pub product_release: Option<ProductReleasePins>,
}

/// Exact tools used by the hosted MBX cache qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbxQualificationPins {
    /// Resolved Mise action and binary pins.
    pub mise_setup: MiseSetup,
    /// Full-SHA candidate MBX Action ref for this unqualified experiment.
    /// It is separate from the production pin and generation never qualifies it.
    pub candidate_action_uses: String,
    /// Exact MBX tool version.
    pub mbx_version: String,
    /// Exact Rust toolchain version used by the qualification lane.
    pub rust_version: String,
}

/// Exact Mise release pins for the hosted Linux and macOS x64 qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MisePinQualificationPins {
    /// Linux x86-64 release asset verification pins.
    pub linux_x86_64_setup: MiseSetup,
    /// macOS x86-64 release asset verification pins.
    pub macos_x86_64_setup: MiseSetup,
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
    RunnerSpec::hosted(&request.hosted_label)?;
    let mut files = Vec::new();
    if request.workflows.contains(&RoutingWorkflow::Qualification) {
        files.push(file(
            QUALIFICATION_WORKFLOW,
            &request.version,
            &qualification(request)?,
        )?);
    }
    if let Some(generated) = product_release::render(request)? {
        files.push(file(
            PRODUCT_RELEASE_WORKFLOW,
            &request.version,
            &generated.workflow,
        )?);
        for (path, workflow) in generated.family_workflows {
            files.push(file(&path, &request.version, &workflow)?);
        }
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
    let bytes = crate::render::fallback::render_checked_workflow(path, body, version)?;
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes,
    })
}

fn qualification(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
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
    if let Some(pins) = &request.mise_pin_qualification {
        jobs.extend(mise_pin_qualification::jobs(pins)?);
    } else if request.workflows.contains(&RoutingWorkflow::Qualification) {
        return Err(RenderError::InvalidWorkflow(
            "missing_mise_pin_qualification_pins".to_owned(),
        ));
    }
    Ok(document("Qualification", mode_trigger(), jobs))
}

fn with_if((id, body): (String, Yaml), when: &str) -> (String, Yaml) {
    let Yaml::Map(mut fields) = body else {
        return (id, body);
    };
    fields.insert(1, ("if".to_owned(), Yaml::str(when)));
    (id, Yaml::Map(fields))
}

fn monitoring(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
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
