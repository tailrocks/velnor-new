//! Render a typed [`RunsOn`] value. Hosted stays a scalar string.

use velnor_actions_contract_config::{RunsOn, SCALE_SET_NAME};
use velnor_actions_contract_release::ReleaseTarget;

use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

/// Scale Set image shell for the active Ubuntu 26.04 linux/amd64 profile.
/// The runner specification pins this base image and it includes Bash.
pub(crate) const SCALE_SET_RUN_SHELL: &str = "bash -e {0}";
/// GitHub Actions job containers default `run` steps to POSIX `sh`.
pub(crate) const CONTAINER_RUN_SHELL: &str = "sh -e {0}";

/// Release triple for a hosted label or the qualified linux scale set.
pub(crate) fn target_for_runner(label: &str) -> Option<&'static str> {
    match RunsOn::parse(label).ok()? {
        RunsOn::Hosted(label) => ReleaseTarget::for_runner_label(&label).map(ReleaseTarget::triple),
        RunsOn::ScaleSet(selector) if selector.name() == SCALE_SET_NAME => {
            Some(ReleaseTarget::LinuxX86_64.triple())
        }
        RunsOn::ScaleSet(_) => None,
    }
}

/// YAML for one job `runs-on` value.
///
/// # Errors
///
/// Illegal labels fail. The scale-set form is a flow sequence in
/// render order, never a free-form string.
pub(crate) fn runs_on_yaml(text: &str) -> Result<Yaml, RenderError> {
    let typed = RunsOn::parse(text).map_err(|err| RenderError::InvalidWorkflow(err.to_string()))?;
    Ok(match typed {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    })
}

/// One job-level run-shell default, shared by the schema-2 and IR renderers.
pub(crate) fn run_shell_defaults_field(shell: &str) -> (String, Yaml) {
    (
        "defaults".to_owned(),
        Yaml::Map(vec![(
            "run".to_owned(),
            Yaml::Map(vec![("shell".to_owned(), Yaml::str(shell.to_owned()))]),
        )]),
    )
}
