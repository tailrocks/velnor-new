use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(super) fn for_roots(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<(Vec<Step>, BTreeMap<String, String>), OrchestratorError> {
    let env = crate::workflow::verifier_actions::producer_cargo_env(catalog)?;
    let steps = crate::source_prep::fetch_steps_for_preview(catalog, roots, &env)?;
    Ok((steps, env))
}
