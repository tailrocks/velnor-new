use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ProposedTask, Stack};
use velnor_actions_mise::{
    CandidateBuild, IsolatedCommand, PinnedTool, PinnedToolExec, RouteDriver, ToolCatalog,
    custom_run::custom_task_run_argv, validate_exact_version,
};
use velnor_actions_rust::tool_needs;
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::{OrchestratorError, qualify::QualifyRequest};

pub(crate) const CARGO_DENY_VERSION: &str = "0.20.2";

pub(crate) const CARGO_MACHETE_VERSION: &str = "0.9.2";
pub(crate) const CARGO_MACHETE_TOOL_SPEC: &str = concat!(
    "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/",
    "download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,",
    "checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]",
);

pub(crate) const LYCHEE_VERSION: &str = "0.15.1";
pub(crate) const LYCHEE_TOOL_SPEC: &str = concat!(
    "http:lychee[url=https://github.com/lycheeverse/lychee/releases/",
    "download/v0.15.1/lychee-v0.15.1-x86_64-unknown-linux-gnu.tar.gz,",
    "checksum=sha256:9afdf0f48064fdcd8a8815ca6d2d9fcd43a0ef4c2edf9da23d6f7bf46a764eb8]",
);

const VALIDATOR_TOOL_SPECS: [&str; 5] = [
    "cargo-deny",
    CARGO_MACHETE_TOOL_SPEC,
    "npm:markdownlint-cli2",
    "node",
    LYCHEE_TOOL_SPEC,
];

const MACHETE_SCAN_CRATES: [&str; 11] = [
    "crates/velnor-actions-contract",
    "crates/velnor-actions-rust",
    "crates/velnor-actions-tofu",
    "crates/velnor-actions-ansible",
    "crates/velnor-actions-dbt",
    "crates/velnor-actions-gradle",
    "crates/velnor-actions-mise",
    "crates/velnor-actions-actionlint",
    "crates/velnor-actions-workflow-renderer",
    "crates/velnor-actions-orchestrator",
    "crates/velnor-actions-cli",
];

pub(crate) fn task_argv(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    match Stack::from_id(&task.stack_id) {
        Some(Stack::Tofu) => return tofu_task_argv(task, catalog),
        Some(Stack::Mise) => {
            let name = task
                .payload
                .first()
                .and_then(|v| v.to_str())
                .filter(|_| task.payload.len() == 1)
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "named_check_task_payload".to_owned(),
                })?;
            return custom_task_run_argv(name).map_err(|e| OrchestratorError::Contract {
                problem: e.to_string(),
            });
        }
        Some(Stack::Native) => {
            return Err(OrchestratorError::Contract {
                problem: "native_homebrew_check_has_no_mise_argv".to_owned(),
            });
        }
        Some(Stack::Rust) => {}
        None => {
            return Err(OrchestratorError::Contract {
                problem: format!("unknown_task_stack:{}", task.stack_id),
            });
        }
    }
    let driver =
        RouteDriver::from_compile_driver(&task.identity.compile_driver).ok_or_else(|| {
            OrchestratorError::Contract {
                problem: format!("unknown_compile_driver:{}", task.identity.compile_driver),
            }
        })?;
    let mut tools = driver.probe_tools();
    if tool_needs(&task.identity.compile_driver, &task.identity.test_runner).nextest {
        tools.push(PinnedTool::Nextest);
    }
    let program = OsString::from(driver.program());
    let exec = PinnedToolExec::new(tools, &program, task.payload.clone()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

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

pub(crate) fn exec_argv(
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
    let inner_command = join_quoted_argv(&inner)?;
    let install_command = join_quoted_argv(&install_argv)?;
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
                inner_command,
                crate::source_prep::isolated_manifest_flag(workspace)
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(crate::source_prep::privilege_drop_argv(
        &install_command,
        &payload,
    ))
}

fn join_quoted_argv(argv: &[String]) -> Result<String, OrchestratorError> {
    argv.iter()
        .map(|element| velnor_actions_workflow_renderer::quote_run_arg(element))
        .collect::<Result<Vec<_>, _>>()
        .map(|words| words.join(" "))
        .map_err(OrchestratorError::from)
}

pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec!["machete"];
    args.extend(MACHETE_SCAN_CRATES);
    validator_argv(
        CARGO_MACHETE_TOOL_SPEC,
        CARGO_MACHETE_VERSION,
        "cargo",
        &args,
    )
}

pub(crate) fn machete_install_argv() -> Result<Vec<String>, OrchestratorError> {
    let install = IsolatedCommand::mise_install(&[format!(
        "{CARGO_MACHETE_TOOL_SPEC}@{CARGO_MACHETE_VERSION}"
    )])
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(install.argv()).map_err(|problem| OrchestratorError::Contract { problem })
}

pub(crate) fn validator_argv(
    spec: &str,
    version: &str,
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, OrchestratorError> {
    validator_argv_parts(spec, version, program, args.iter().copied())
}

pub(crate) fn validator_argv_owned(
    spec: &str,
    version: &str,
    program: &str,
    args: &[String],
) -> Result<Vec<String>, OrchestratorError> {
    validator_argv_parts(spec, version, program, args.iter().map(String::as_str))
}

fn validator_argv_parts<'a>(
    spec: &str,
    version: &str,
    program: &'a str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, OrchestratorError> {
    if !VALIDATOR_TOOL_SPECS.contains(&spec) {
        return Err(OrchestratorError::Contract {
            problem: format!("validator_tool_rejected:{spec}"),
        });
    }
    validate_exact_version(spec, version).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let payload: Vec<OsString> = std::iter::once(program)
        .chain(args)
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

pub(crate) fn candidate_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let build = CandidateBuild::new().map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(build.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

pub(crate) fn candidate_spec() -> Result<CandidateSpec, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    Ok(CandidateSpec {
        build: candidate_build_argv(&catalog)?,
        qualify: QualifyRequest::staged().argv()?,
    })
}

fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}
