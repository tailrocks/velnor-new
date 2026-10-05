//! V1 fixed command vectors built only through the Mise adapter.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ProposedTask, Stack, Step};
use velnor_actions_mise::{
    CandidateBuild, IsolatedCommand, PinnedTool, PinnedToolExec, RouteDriver, ToolCatalog,
    custom_run::custom_task_run_argv, validate_exact_version,
};
use velnor_actions_rust::tool_needs;
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::{OrchestratorError, qualify::QualifyRequest};

/// Qualified cargo-deny release.
/// Source: `https://crates.io/api/v1/crates/cargo-deny`; checked 2026-09-29.
/// The mise registry shorthand `cargo-deny` resolves it (aqua backend); the
/// isolated `mise exec cargo-deny@0.20.2 -- cargo deny --version` probe
/// reported cargo-deny 0.20.2.
pub(crate) const CARGO_DENY_VERSION: &str = "0.20.2";

/// Resolve an emitted validator install spec to its pinned name and version.
///
/// Only `cargo-deny` installs (machete and zizmor run through isolated
/// `exec`, never `install`); the version must equal the pinned const or
/// the emitted shape drifted and the audit fails closed.
#[must_use]
pub(crate) fn validator_install_pin(spec: &str) -> Option<(&'static str, &'static str)> {
    let (key, version) = spec.split_once('@')?;
    if key == "cargo-deny" && version == CARGO_DENY_VERSION {
        Some(("cargo-deny", CARGO_DENY_VERSION))
    } else {
        None
    }
}

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
const MACHETE_SCAN_CRATES: [&str; 8] = [
    "crates/velnor-actions-contract",
    "crates/velnor-actions-rust",
    "crates/velnor-actions-tofu",
    "crates/velnor-actions-mise",
    "crates/velnor-actions-actionlint",
    "crates/velnor-actions-workflow-renderer",
    "crates/velnor-actions-orchestrator",
    "crates/velnor-actions-cli",
];

/// Contract-fixed display name of the policy zizmor step.
pub(crate) const ZIZMOR_STEP_NAME: &str = "Run zizmor";

/// V1 fixed vector for one task: pinned `mise` payload plus kind args.
///
/// Program follows the compile route (`mbx` for MBX, `cargo` otherwise); Nextest
/// profiles add the runner tool, `cargo_test` legs never carry it. Tofu
/// tasks route to the pinned `opentofu` tool invoking `tofu`. The
/// payload is the adapter-precomputed command, wrapped never edited.
pub(crate) fn task_argv(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        return tofu_task_argv(task, catalog);
    }
    let driver = RouteDriver::from_compile_driver(&task.identity.compile_driver);
    let mut tools = driver.map_or(vec![PinnedTool::Rust], RouteDriver::tools);
    if tool_needs(&task.identity.compile_driver, &task.identity.test_runner).nextest {
        tools.push(PinnedTool::Nextest);
    }
    let program = OsString::from(driver.map_or("cargo", RouteDriver::program));
    let exec = PinnedToolExec::new(tools, &program, task.payload.clone()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// V1 fixed vector for one tofu task: pinned `opentofu`, program `tofu`.
///
/// The fixed payload rides through wrapped, never edited.
fn tofu_task_argv(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Opentofu],
        OsStr::new("tofu"),
        task.payload.clone(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
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

/// Fixed validator-job vector: privilege-dropping `cargo deny` for each
/// repository-owned Cargo workspace.
///
/// Deny shells `cargo metadata`, which reads repo `.cargo/config.toml`
/// even from outside the checkout (deny resolves config from the
/// manifest side), so cwd isolation alone cannot starve repo
/// credential-providers. The script therefore drops privilege first:
/// an ambient-credential `mise install` (isolation trio, so no repo
/// config loads) warms the pinned tool, then the shared
/// credential-unset prelude removes every ambient secret, and only
/// then does the isolated deny payload run for both the generator and
/// nested runner workspaces. Each invocation supplies that workspace's
/// absolute manifest and deny configuration before the trailing `check`:
/// deny takes global flags pre-subcommand. A repo provider executing past
/// this point observes an empty credential environment. A drifted payload
/// shape fails closed instead of splicing into the wrong position.
/// Shell assembly lives in [`crate::source_prep::privilege_drop_argv`]:
/// this module composes typed argv only, keeping the orchestrator's
/// `sh` confinement set closed.
pub(crate) fn deny_argv(workspace_roots: &[String]) -> Result<Vec<String>, OrchestratorError> {
    let mut roots = workspace_roots.to_vec();
    roots.sort();
    roots.dedup();
    if roots.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "deny_requires_workspace".to_owned(),
        });
    }
    for root in &roots {
        crate::source_prep::validate_root(root)?;
    }
    let mut inner = validator_argv(
        "cargo-deny",
        CARGO_DENY_VERSION,
        "cargo",
        &["deny", "--locked", "check"],
    )?;
    if inner.pop().as_deref() != Some("check") {
        return Err(OrchestratorError::Contract {
            problem: "deny_vector_shape_drift".to_owned(),
        });
    }
    let install = IsolatedCommand::mise_install(&[format!("cargo-deny@{CARGO_DENY_VERSION}")])
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let install_argv =
        strings_of(install.argv()).map_err(|problem| OrchestratorError::Contract { problem })?;
    let payload = roots
        .iter()
        .map(|workspace| {
            let config = if workspace.is_empty() {
                "deny.toml".to_owned()
            } else {
                format!("{workspace}/deny.toml")
            };
            format!(
                "{} {} --config \"$GITHUB_WORKSPACE/{config}\" check",
                join_quoted_argv(&inner),
                crate::source_prep::isolated_manifest_flag(workspace)
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(crate::source_prep::privilege_drop_argv(
        &join_quoted_argv(&install_argv),
        &payload,
    ))
}

/// Join fixed argv elements with the renderer's POSIX quoter.
///
/// Every current element is a plain token (identity join); quoting
/// through the one authority keeps future drift exact instead of
/// silently mis-spliced.
fn join_quoted_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|element| velnor_actions_workflow_renderer::quote_run_arg(element))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Policy zizmor scan target: generated workflows only.
///
/// Never the repo root: `fixtures/` carries intentional negative
/// workflows that must fail adapter tests, not the policy audit.
const ZIZMOR_POLICY_INPUT: &str = ".github/workflows";

/// Policy zizmor config: the committed zero-ignore policy file.
///
/// Every emitted ref is hash-pinned, so the `unpinned-uses` ignore list
/// is empty; a missing file errors the scan instead of silently dropping
/// the policy.
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

/// Shell steps running each allowlisted custom task via `mise run`.
///
/// Rejected while non-empty: the emitted steps cannot work as built
/// (the step env carries `MISE_NO_CONFIG=1`, under which mise reports
/// `no tasks defined`), so any non-empty allowlist fails `generate`
/// instead of shipping dead steps. Enabling this path needs a
/// redesign (a qualified config-visible execution boundary plus the
/// corrected `mise run <task>` argv), not a flag flip: dropping
/// `MISE_NO_CONFIG` from the step env would let repository task
/// bodies execute with the job's credentials and checkout. An empty
/// allowlist (the default) emits nothing.
/// # Errors
///
/// Returns `custom_tasks_unqualified` for any non-empty allowlist, or
/// a contract error when a name fails the task-name rule.
pub(crate) fn custom_task_steps(
    allowlist: &[String],
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, OrchestratorError> {
    if !allowlist.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "custom_tasks_unqualified:custom_tasks is rejected until the config-visible execution path is redesigned; see custom_task_steps docs"
                .to_owned(),
        });
    }
    allowlist
        .iter()
        .map(|task| {
            let run = custom_task_run_argv(task).map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?;
            let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
            velnor_actions_workflow_renderer::shell_step(&format!("Custom task {task}"), run, env)
                .map_err(|err| OrchestratorError::Contract {
                    problem: err.to_string(),
                })
        })
        .collect()
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
