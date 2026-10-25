use velnor_actions_contract::{ReleaseTarget, RunsOn, SCALE_SET_NAME};

use crate::{RenderError, yaml::Yaml};

pub(crate) fn target_for_runner(label: &str) -> Option<&'static str> {
    match RunsOn::parse(label).ok()? {
        RunsOn::Hosted(label) => ReleaseTarget::for_runner_label(&label).map(ReleaseTarget::triple),
        RunsOn::ScaleSet(selector) if selector.name() == SCALE_SET_NAME => {
            Some(ReleaseTarget::LinuxX86_64.triple())
        }
        RunsOn::ScaleSet(_) => None,
    }
}

pub(crate) const SCALE_SET_RUN_SHELL: &str = "bash -e {0}";
pub(crate) const CONTAINER_RUN_SHELL: &str = "sh -e {0}";

pub(crate) fn runs_on_yaml(text: &str) -> Result<Yaml, RenderError> {
    let typed = RunsOn::parse(text).map_err(|err| RenderError::InvalidWorkflow(err.to_string()))?;
    Ok(match typed {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    })
}

pub(crate) fn run_shell_defaults_field(shell: &str) -> (String, Yaml) {
    (
        "defaults".to_owned(),
        Yaml::Map(vec![(
            "run".to_owned(),
            Yaml::Map(vec![("shell".to_owned(), Yaml::str(shell.to_owned()))]),
        )]),
    )
}
