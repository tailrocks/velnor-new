//! Required Python source-unit suites for the Velnor repository.

use std::ffi::OsString;

use velnor_actions_contract::ValidatorKind;
use velnor_actions_mise::{IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{
    PYTHON_SOURCE_RUN_NAME, ValidatorCommand, ValidatorSourceInput,
    ValidatorSourceInputKind as InputKind, ValidatorSourceUnit,
};

use crate::OrchestratorError;

struct SuiteSpec {
    id: &'static str,
    discovery_root: &'static str,
    pattern: Option<&'static str>,
    inputs: &'static [(&'static str, InputKind)],
}

const OBSERVER_INPUTS: &[(&str, InputKind)] = &[
    (
        "scripts/qualification/mbx-synchronous/artifact_closure_v2.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/bind_inputs.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/cache_transport_v2.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/manifest.json",
        InputKind::Fixture,
    ),
    (
        "scripts/qualification/mbx-synchronous/negative_inputs_v2.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/run.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/run_negative_v2.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/run_same_root.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/run_v2.py",
        InputKind::Module,
    ),
    (
        "scripts/qualification/mbx-synchronous/test_negative_pipeline_v2.py",
        InputKind::Test,
    ),
    (
        "scripts/qualification/mbx-synchronous/test_negative_v2.py",
        InputKind::Test,
    ),
    (
        "scripts/qualification/mbx-synchronous/test_observer_artifact_closure.py",
        InputKind::Test,
    ),
    (
        "scripts/qualification/mbx-synchronous/test_run_same_root.py",
        InputKind::Test,
    ),
    (
        "scripts/qualification/mbx-synchronous/test_run_v2.py",
        InputKind::Test,
    ),
    (
        "crates/mbx-synchronous-registry-fixture/src/lib.rs",
        InputKind::Fixture,
    ),
    (
        "crates/mbx-synchronous-registry-fixture/tests/fixtures/standalone-workspace-v1.json",
        InputKind::Fixture,
    ),
];

const COLLECTOR_INPUTS: &[(&str, InputKind)] = &[
    (
        "scripts/hosted_phase_measurement/__init__.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/hosted_dag.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/mbx_summary.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/measure_hosted_run.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/measure_support.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/report_builder.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/report_counters.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/report_events.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/step_phases.py",
        InputKind::Module,
    ),
    (
        "scripts/hosted_phase_measurement/tests/fixtures/mbx-summary-exact-joined-v1.jsonl",
        InputKind::Fixture,
    ),
    (
        "scripts/hosted_phase_measurement/tests/fixtures/runner-core-step-49-post-restore-mbx-objects.txt",
        InputKind::Fixture,
    ),
    (
        "scripts/hosted_phase_measurement/tests/test_mbx_summary.py",
        InputKind::Test,
    ),
    (
        "scripts/hosted_phase_measurement/tests/test_measure_hosted_run.py",
        InputKind::Test,
    ),
];

const FRESHNESS_PROBE_INPUTS: &[(&str, InputKind)] = &[
    ("scripts/freshness_probe.py", InputKind::Module),
    ("scripts/test_freshness_probe.py", InputKind::Test),
];

const FRESHNESS_PARSER_INPUTS: &[(&str, InputKind)] = &[
    ("scripts/freshness_probe_check.py", InputKind::Module),
    ("scripts/test_freshness_probe_check.py", InputKind::Test),
];

const SUITES: [SuiteSpec; 4] = [
    SuiteSpec {
        id: "mbx-synchronous-observer",
        discovery_root: "scripts/qualification/mbx-synchronous",
        pattern: None,
        inputs: OBSERVER_INPUTS,
    },
    SuiteSpec {
        id: "hosted-phase-measurement",
        discovery_root: "scripts/hosted_phase_measurement/tests",
        pattern: None,
        inputs: COLLECTOR_INPUTS,
    },
    SuiteSpec {
        id: "freshness-probe",
        discovery_root: "scripts",
        pattern: Some("test_freshness_probe.py"),
        inputs: FRESHNESS_PROBE_INPUTS,
    },
    SuiteSpec {
        id: "freshness-parser",
        discovery_root: "scripts",
        pattern: Some("test_freshness_probe_check.py"),
        inputs: FRESHNESS_PARSER_INPUTS,
    },
];

const TOOL_INPUTS: &[(&str, InputKind)] = &[
    (".mise-version", InputKind::Configuration),
    (".velnor/version-policy.toml", InputKind::Configuration),
    (
        "crates/velnor-actions-mise/src/catalog.rs",
        InputKind::ToolchainPin,
    ),
    (
        "crates/velnor-actions-mise/src/catalog_versions.rs",
        InputKind::ToolchainPin,
    ),
];

/// Build the required Python source-suite command and pinned install vector.
pub(crate) fn validator_command(
    catalog: &ToolCatalog,
) -> Result<ValidatorCommand, OrchestratorError> {
    let python = catalog.tool_spec(PinnedTool::Python);
    let prepare = IsolatedCommand::mise_install(std::slice::from_ref(&python)).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let prepare_argv =
        strings_of(prepare.argv()).map_err(|problem| OrchestratorError::Contract { problem })?;
    let commands: Vec<String> = SUITES
        .iter()
        .map(|suite| {
            let pattern = suite.pattern.map_or_else(String::new, |pattern| {
                format!(" -p '{pattern}'")
            });
            format!(
                "mise --no-config --no-env --no-hooks exec {python} -- python -B -m unittest discover -s {}{pattern} -v",
                suite.discovery_root
            )
        })
        .collect();
    let script = format!(
        "set -eu; export MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false MISE_LOCKFILE=0 MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1; {}",
        commands.join(" && ")
    );
    Ok(ValidatorCommand {
        validator: ValidatorKind::PythonSourceTests,
        name: PYTHON_SOURCE_RUN_NAME.to_owned(),
        argv: vec!["sh".to_owned(), "-c".to_owned(), script],
        prepare_argv,
        source_units: source_units(),
        tool_inputs: inputs(TOOL_INPUTS),
    })
}

/// Materialize the fixed source suites and their complete input closures.
fn source_units() -> Vec<ValidatorSourceUnit> {
    SUITES
        .iter()
        .map(|suite| ValidatorSourceUnit {
            id: suite.id.to_owned(),
            discovery_root: suite.discovery_root.to_owned(),
            pattern: suite.pattern.map(str::to_owned),
            inputs: inputs(suite.inputs),
        })
        .collect()
}

/// Convert one static input table to renderer-owned typed inputs.
fn inputs(values: &[(&str, InputKind)]) -> Vec<ValidatorSourceInput> {
    values
        .iter()
        .map(|(path, kind)| ValidatorSourceInput {
            path: (*path).to_owned(),
            kind: *kind,
        })
        .collect()
}

/// Convert a pinned Mise argv without lossy argument conversion.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}

#[cfg(test)]
#[path = "python_source_units_tests.rs"]
mod tests;
