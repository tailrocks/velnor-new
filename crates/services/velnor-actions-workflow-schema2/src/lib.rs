//! Schema-2 workflows: qualification, image/macOS/generator release, monitoring.
//!
//! Pure and total: schema-2 emission contains no `std::fs`, `std::net`, or
//! process calls. Callers supply all discovered inputs.

#![forbid(unsafe_code)]

use velnor_actions_contract_config::config::is_hosted_catalog;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunnerLane {
    Hosted,
    ScaleSet,
}

pub(crate) struct RunnerSpec {
    pub(crate) runs_on: Yaml,
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

    pub(crate) fn push_default_shell(&self, fields: &mut Vec<(String, Yaml)>, has_container: bool) {
        if has_container {
            fields.push(
                velnor_actions_workflow_tree::runs_on::run_shell_defaults_field(
                    velnor_actions_workflow_tree::runs_on::CONTAINER_RUN_SHELL,
                ),
            );
        } else if self.lane == RunnerLane::ScaleSet {
            fields.push(
                velnor_actions_workflow_tree::runs_on::run_shell_defaults_field(
                    velnor_actions_workflow_tree::runs_on::SCALE_SET_RUN_SHELL,
                ),
            );
        }
    }
}

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
/// Image-release workflow path.
pub const IMAGE_RELEASE_WORKFLOW: &str = ".github/workflows/image-release.yml";
/// macOS binary-release workflow path.
pub const MACOS_BINARY_RELEASE_WORKFLOW: &str = ".github/workflows/macos-binary-release.yml";
/// Generator-release workflow path.
pub const GENERATOR_RELEASE_WORKFLOW: &str = ".github/workflows/generator-release.yml";
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";

mod classes;
mod features;
mod mbx_qualification;
mod release;
/// Exact-source gates for composed product-release workflows.
pub mod release_eligibility;
mod workflows;

pub use workflows::render_schema2_workflows;

#[cfg(test)]
mod tests;
