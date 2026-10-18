//! Credential-free P08 preparation for preview runtime's offline discovery.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(super) fn for_roots(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<(Vec<Step>, BTreeMap<String, String>), OrchestratorError> {
    let mut env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    for (native, mise) in [
        ("CARGO_HOME", "MISE_CARGO_HOME"),
        ("RUSTUP_HOME", "MISE_RUSTUP_HOME"),
    ] {
        env.insert(native.to_owned(), env[mise].clone());
    }
    let steps = crate::source_prep::fetch_steps_for_preview(catalog, roots, &env)?;
    Ok((steps, env))
}

#[cfg(test)]
#[path = "preview_source_prep_tests.rs"]
mod tests;
