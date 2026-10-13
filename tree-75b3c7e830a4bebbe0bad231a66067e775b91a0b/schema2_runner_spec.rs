//! Runner selection and shell defaults for schema 2 workflows.

use velnor_actions_contract::{ReleaseTarget, config::is_hosted_catalog};

use crate::RenderError;
use crate::yaml::Yaml;

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
    pub(super) fn hosted(label: &str) -> Result<Self, RenderError> {
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

    /// Hosted runner selected by a native release target.
    ///
    /// Qualification may use the typed release-target catalog for macOS
    /// x64 even though that runner is not a configurable CI label.
    pub(super) fn hosted_release_target(
        label: &str,
        target: ReleaseTarget,
    ) -> Result<Self, RenderError> {
        let allowed_pair = matches!(
            (label, target),
            ("ubuntu-26.04", ReleaseTarget::LinuxX86_64)
                | ("macos-15-intel", ReleaseTarget::MacosX86_64)
        );
        if !allowed_pair || ReleaseTarget::for_runner_label(label) != Some(target) {
            return Err(RenderError::InvalidWorkflow(format!(
                "schema2_hosted_runner_target_mismatch:{label}:{}",
                target.triple()
            )));
        }
        Ok(Self {
            runs_on: Yaml::str(label),
            lane: RunnerLane::Hosted,
        })
    }

    pub(super) fn scale_set(runs_on: Yaml) -> Self {
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

#[cfg(test)]
#[path = "schema2_runner_spec_tests.rs"]
mod tests;
