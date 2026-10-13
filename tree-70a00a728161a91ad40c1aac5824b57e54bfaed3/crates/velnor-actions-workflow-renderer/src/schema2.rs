//! Qualification, composed product-release, and monitoring workflows.
//! Emitted only when schema 2 requests them.

use std::collections::BTreeSet;

use velnor_actions_contract::config::is_hosted_catalog;
use velnor_actions_contract::{
    ReleaseTarget, RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL,
};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::runs_on::runs_on_yaml;
use crate::setup::MiseSetup;
use crate::yaml::{Yaml, render_yaml};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerLane {
    Hosted,
    ScaleSet,
}

pub(super) struct RunnerSpec {
    pub(super) runs_on: Yaml,
    lane: RunnerLane,
}

impl RunnerSpec {
    fn hosted(label: &str) -> Result<Self, RenderError> {
        if !is_hosted_catalog(label) {
            return Err(RenderError::InvalidWorkflow(format!(
                "schema2_hosted_runner_not_catalog:{label}"
            )));
        }
        Ok(Self {
            runs_on: Yaml::str(label),
            lane: RunnerLane::Hosted,
        })
    }

    fn scale_set(runs_on: Yaml) -> Self {
        Self {
            runs_on,
            lane: RunnerLane::ScaleSet,
        }
    }

    pub(super) fn push_default_shell(&self, fields: &mut Vec<(String, Yaml)>, has_container: bool) {
        if has_container {
            fields.push(crate::runs_on::run_shell_defaults_field(
                crate::runs_on::CONTAINER_RUN_SHELL,
            ));
        } else if self.lane == RunnerLane::ScaleSet {
            fields.push(crate::runs_on::run_shell_defaults_field(
                crate::runs_on::SCALE_SET_RUN_SHELL,
            ));
        }
    }
}

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
/// Shared product-release workflow path.
pub const PRODUCT_RELEASE_WORKFLOW: &str = product_release::WORKFLOW_PATH;
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";

#[path = "schema2_classes.rs"]
mod classes;
#[path = "schema2_features.rs"]
mod features;
#[path = "schema2_generator_release.rs"]
mod generator_release;
#[path = "schema2_mbx_qualification.rs"]
mod mbx_qualification;
#[path = "schema2_product_release.rs"]
mod product_release;
#[path = "schema2_product_release_family.rs"]
mod product_release_family;
#[path = "schema2_release.rs"]
mod release;
/// Exact-source gates for composed product-release workflows.
#[path = "schema2_release_eligibility.rs"]
pub mod release_eligibility;

#[cfg(test)]
#[path = "schema2_runner_shell_tests.rs"]
mod runner_shell_tests;

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
    /// Orchestrator-resolved Mise setup and command vectors for product releases.
    pub product_release: Option<ProductReleasePins>,
}

/// Pinned tools and runner-specific Mise setup for composed product releases.
///
/// The orchestrator builds every command vector through the Mise adapter.
/// The renderer only joins validated argv into workflow steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReleasePins {
    /// Setup pins for each supported release runner target.
    pub linux_x86_64_setup: MiseSetup,
    /// Setup pins for each supported release runner target.
    pub macos_arm64_setup: MiseSetup,
    /// Setup pins for each supported release runner target.
    pub macos_x86_64_setup: MiseSetup,
    /// Exact `mise install` argv for source-policy tools.
    pub install_gate_tools_argv: Vec<String>,
    /// Exact `mise install` argv for native candidate build tools.
    pub install_build_tools_argv: Vec<String>,
    /// Exact `mise install` argv for the macOS host binary build.
    pub install_runner_build_tools_argv: Vec<String>,
    /// Exact `mise install` argv for GitHub CLI.
    pub install_gh_argv: Vec<String>,
    /// Exact pinned `mbx build` argv.
    pub build_argv: Vec<String>,
    /// Exact pinned `mbx build` argv for the Intel cross-compile leg.
    pub intel_build_argv: Vec<String>,
    /// Exact `rustup target add` argv for the Intel target std.
    pub install_intel_target_argv: Vec<String>,
    /// Exact pinned macOS host binary build argv.
    pub runner_build_argv: Vec<String>,
    /// Exact pinned actionlint argv.
    pub actionlint_argv: Vec<String>,
    /// Exact pinned zizmor argv.
    pub zizmor_argv: Vec<String>,
    /// Exact pinned GitHub CLI invocation prefix.
    pub gh_argv: Vec<String>,
    /// Exact Rust toolchain release selected by the Mise catalog.
    pub rust_version: String,
    /// Exact MBX release selected by the Mise catalog.
    pub mr_boxington_version: String,
}

impl ProductReleasePins {
    /// Setup action pins associated with a release target.
    #[must_use]
    pub fn setup_for(&self, target: ReleaseTarget) -> &MiseSetup {
        match target {
            ReleaseTarget::LinuxX86_64 => &self.linux_x86_64_setup,
            ReleaseTarget::MacosArm64 => &self.macos_arm64_setup,
            ReleaseTarget::MacosX86_64 => &self.macos_x86_64_setup,
        }
    }
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
    let bytes = with_marker(version, &render_yaml(body))?;
    crate::workflow_size::check_workflow_size(path, &bytes)?;
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
