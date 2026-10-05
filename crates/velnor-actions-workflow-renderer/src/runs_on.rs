//! Render a typed [`RunsOn`] value. Hosted stays a scalar string.

use velnor_actions_contract::RunsOn;

use crate::{RenderError, yaml::Yaml};

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
