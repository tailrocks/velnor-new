//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::{OsStr, OsString};

use velnor_actions_mise::{
    CandidateBuild, IsolatedCommand, PinnedTool, PinnedToolExec, RouteDriver, ToolCatalog,
    validate_exact_version,
};
use velnor_actions_rust::{
    NextestProfile, TaskGroup, TestRunner, tasks::cargo_payload_with_profile,
};
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::{OrchestratorError, qualify::QualifyRequest};

/// Qualified cargo-deny release.
/// Source: `https://crates.io/api/v1/crates/cargo-deny`; checked 2026-09-29.
/// The mise registry shorthand `cargo-deny` resolves it (aqua backend); the
/// isolated `mise exec cargo-deny@0.20.2 -- cargo deny --version` probe
/// reported cargo-deny 0.20.2.
const CARGO_DENY_VERSION: &str = "0.20.2";

/// Qualified cargo-machete release.
/// Source: `https://crates.io/api/v1/crates/cargo-machete`; checked 2026-09-29.
/// The mise registry has no `cargo-machete` shorthand and the aqua registry
/// has no package, so the spec is backend-qualified `ubi:` (same precedent
/// as Nextest's aqua path): `mise ls-remote ubi:bnjbvr/cargo-machete` lists
/// 0.9.2 and the isolated `mise exec ubi:bnjbvr/cargo-machete@0.9.2 --
/// cargo machete --version` probe reported 0.9.2.
const CARGO_MACHETE_VERSION: &str = "0.9.2";

/// Mise tool specs the validator vectors may select, without versions.
const VALIDATOR_TOOL_SPECS: [&str; 2] = ["cargo-deny", "ubi:bnjbvr/cargo-machete"];

/// Product crates scanned by the machete vector, in contract order.
///
/// Fixed paths keep the scan hermetic: it covers exactly the product
/// crates, never fixtures or tooling trees (a bare walk previously
/// errored on a symlink-hazard fixture; hazards now live only in
/// TempDir-built tests, never in the tree).
const MACHETE_SCAN_CRATES: [&str; 7] = [
    "crates/velnor-actions-contract",
    "crates/velnor-actions-rust",
    "crates/velnor-actions-mise",
    "crates/velnor-actions-actionlint",
    "crates/velnor-actions-workflow-renderer",
    "crates/velnor-actions-orchestrator",
    "crates/velnor-actions-cli",
];

/// Contract-fixed display name of the policy zizmor step.
pub(crate) const ZIZMOR_STEP_NAME: &str = "Run zizmor";

/// V1 fixed vector for one group: pinned `mise` payload plus kind args.
///
/// Program follows the compile route (`mbx` for MBX, `cargo` otherwise); Nextest
/// profiles add the runner tool, `cargo_test` legs never carry it.
pub(crate) fn task_argv(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let driver = RouteDriver::from_compile_driver(&group.compile_driver);
    let mut tools = driver.map_or(vec![PinnedTool::Rust], RouteDriver::tools);
    if group.test_runner == TestRunner::CargoNextest.as_str() {
        tools.push(PinnedTool::Nextest);
    }
    let program = OsString::from(driver.map_or("cargo", RouteDriver::program));
    let profile = NextestProfile::parse(&group.nextest_profile).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let payload = cargo_payload_with_profile(group, profile);
    let exec = PinnedToolExec::new(tools, &program, payload).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed vector: pinned tools plus a literal payload through Mise.
fn exec_argv(
    tools: Vec<PinnedTool>,
    program: &str,
    args: &[&str],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        tools,
        OsStr::new(program),
        args.iter().copied().map(OsString::from).collect(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed validator-job vector: `cargo deny --locked check` through pinned Mise.
pub(crate) fn deny_argv() -> Result<Vec<String>, OrchestratorError> {
    validator_argv(
        "cargo-deny",
        CARGO_DENY_VERSION,
        "cargo",
        &["deny", "--locked", "check"],
    )
}

/// Policy zizmor scan target: generated workflows only.
///
/// Never the repo root: `fixtures/` carries intentional negative
/// workflows that must fail adapter tests, not the policy audit.
const ZIZMOR_POLICY_INPUT: &str = ".github/workflows";

/// Policy zizmor config: the committed reviewed-tag exception file.
///
/// The config carries exactly the version-policy §2 `unpinned-uses`
/// ignore; a missing file errors the scan instead of silently dropping
/// the exception.
const ZIZMOR_POLICY_CONFIG: &str = ".zizmor.yml";

/// Fixed validator-job vector: offline zizmor audit through pinned Mise.
pub(crate) fn zizmor_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        vec![PinnedTool::Zizmor],
        "zizmor",
        &[
            "--no-online-audits",
            "--config",
            ZIZMOR_POLICY_CONFIG,
            ZIZMOR_POLICY_INPUT,
        ],
        catalog,
    )
}

/// Fixed validator-job vector: `cargo machete` over product crates via Mise.
pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec!["machete"];
    args.extend(MACHETE_SCAN_CRATES);
    validator_argv(
        "ubi:bnjbvr/cargo-machete",
        CARGO_MACHETE_VERSION,
        "cargo",
        &args,
    )
}

/// One validator vector: an allowlisted tool spec plus a fixed cargo payload.
///
/// Built through the Mise adapter's isolated `exec` constructor, so the
/// emitted shape (global flags, spec, `--` separator, payload) matches the
/// typed `PinnedToolExec` vectors byte for byte. The spec name must be
/// allowlisted and the version an exact pin; anything else fails closed.
fn validator_argv(
    spec: &str,
    version: &str,
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, OrchestratorError> {
    if !VALIDATOR_TOOL_SPECS.contains(&spec) {
        return Err(OrchestratorError::Contract {
            problem: format!("validator_tool_rejected:{spec}"),
        });
    }
    validate_exact_version(spec, version).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let payload: Vec<OsString> = [program]
        .into_iter()
        .chain(args.iter().copied())
        .map(OsString::from)
        .collect();
    let exec =
        IsolatedCommand::mise_exec(&[format!("{spec}@{version}")], &payload).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    strings_of(exec.argv()).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed pre-seed MBX route probe through pinned Mise.
///
/// Runs `mbx --version` under the exact pinned `mr-boxington` spec so
/// the verify step proves the compile route, not just the output file.
/// Resolves through Mise on every run, cold or warm.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the vector.
pub(crate) fn mbx_probe_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        vec![PinnedTool::MrBoxington],
        "mbx",
        &["--version"],
        catalog,
    )
}

/// Fixed bootstrap §4 build vector through pinned Mise.
///
/// Shared by the candidate build and the pre-seed helper build, so both
/// compile `velnor-actions-cli`/`velnor-actions` with the exact same
/// pinned Rust plus MBX toolchain and flags.
pub(crate) fn candidate_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let build = CandidateBuild::new().map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(build.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed candidate build plus qualification vectors.
pub(crate) fn candidate_spec(catalog: &ToolCatalog) -> Result<CandidateSpec, OrchestratorError> {
    Ok(CandidateSpec {
        build: candidate_build_argv(catalog)?,
        qualify: QualifyRequest::staged().argv()?,
    })
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}

#[cfg(test)]
#[path = "vectors_tests.rs"]
mod vectors_tests;
