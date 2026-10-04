//! Thin composition over the Mise owner's source-bound Rust preparation.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, ToolCatalog,
    catalog::rust_prepare::{RustPrepareDomain, helper_for_install},
};

use crate::OrchestratorError;

/// Normal jobs use the tools domain; early Plan selects its bootstrap explicitly.
pub(crate) fn prepare_step(
    run: Vec<String>,
    env: BTreeMap<String, String>,
    use_rust: bool,
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    prepare_step_in_domain(run, env, use_rust, catalog, RustPrepareDomain::Tools)
}

/// Construct Rust preparation through its sole compiled source owner.
pub(crate) fn prepare_step_in_domain(
    run: Vec<String>,
    mut env: BTreeMap<String, String>,
    use_rust: bool,
    catalog: &ToolCatalog,
    domain: RustPrepareDomain,
) -> Result<Step, OrchestratorError> {
    if use_rust {
        let record = helper_for_install(catalog, domain, &run, env!("CARGO_PKG_VERSION")).map_err(
            |error| OrchestratorError::Contract {
                problem: error.to_string(),
            },
        )?;
        return velnor_actions_workflow_renderer::source_helper::source_helper_step(
            PREPARE_PINNED_TOOLS_STEP,
            &record,
            record.environment().clone(),
        )
        .map_err(OrchestratorError::from);
    }
    let install_at =
        run.iter()
            .position(|arg| arg == "install")
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "missing_prepare_install".to_owned(),
            })?;
    let mut specs = run[install_at + 1..].to_vec();
    specs.sort();
    specs.dedup();
    let identity = velnor_actions_contract::digest_b3(specs.join("\0").as_bytes());
    env.insert(
        "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
        format!("toolset@{identity}"),
    );
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
        .map_err(OrchestratorError::from)
}

#[cfg(test)]
#[path = "rust_tools_prepare_tests.rs"]
mod tests;
