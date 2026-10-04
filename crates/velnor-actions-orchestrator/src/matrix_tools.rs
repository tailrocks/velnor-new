//! Crate-job tool-set construction: drivers plus validator trio.
//!
//! Declared via `#[path]` from `matrix_step.rs` (no `lib.rs` edit);
//! `matrix_step` re-exports the constructors so call sites stay put.

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Crate-job driver tools per role: Rust only when the job carries
/// rust obligations, plus MBX only on MBX evidence, plus Opentofu
/// when the job carries tofu obligations. Pure-tofu jobs install the
/// opentofu driver with no Rust setup; mixed jobs install the union.
#[must_use]
pub(crate) fn task_driver_tools(
    use_rust: bool,
    use_mbx: bool,
    use_opentofu: bool,
) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

#[path = "matrix_suite.rs"]
mod suite;
pub(crate) use suite::{SuiteTools, crate_suite_tools};

/// Select the complete tool set before its preparation source identity is built.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "four independent driver flags precede the audited suite requirements"
)]
pub(crate) fn selected_crate_tools(
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    suite: SuiteTools,
) -> Vec<PinnedTool> {
    let mut tools = task_driver_tools(use_rust, use_mbx, use_opentofu || suite.opentofu);
    if suite.generate_validators {
        tools.extend([
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ]);
    }
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    tools.extend(suite.python.then_some(PinnedTool::Python));
    for tool in &mut tools {
        if matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop) {
            *tool = catalog.compiler_tool();
        }
    }
    tools
}

/// Typed `Prepare pinned tools` step for the crate-job tool set.
///
/// Driver toolchain per role (Rust only for rust obligations, plus
/// Opentofu for tofu jobs) plus Nextest when used, plus the
/// suite tools from the single audited registry (see
/// [`crate_suite_tools`]). The set is exact and pinned
/// by test: drivers, conditional validators, optional Nextest,
/// nothing else. Full preparation carries its canonical home bindings;
/// Rust selectors additionally bind the compiler toolchain.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "four independent driver flags precede the audited suite requirements"
)]
pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    suite: SuiteTools,
    label: &str,
) -> Result<Step, OrchestratorError> {
    let tools = selected_crate_tools(catalog, use_rust, use_mbx, use_nextest, use_opentofu, suite);
    let prepare = PreparePinnedTools::new(tools, ToolHomes::runner_temp()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let run =
        strings_of(prepare.argv_for_host(catalog, crate::workloads::host_for_runner(label)?)?)
            .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if use_rust {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&PreparePinnedTools::env_for_domain(
            velnor_actions_contract::ToolCacheDomain::Full,
        ))
    }
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    crate::workflow_jobs::rust_tools_prepare::prepare_step(run, env, use_rust, catalog)
}
