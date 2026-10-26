// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::SupportCommandError;
use std::ffi::OsString;
use velnor_actions_mise::{IsolatedCommand, validate_exact_version};

pub const CARGO_DENY_VERSION: &str = "0.20.2";

pub const CARGO_MACHETE_VERSION: &str = "0.9.2";
pub const CARGO_MACHETE_TOOL_SPEC: &str = concat!(
    "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/",
    "download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,",
    "checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]",
);

pub const LYCHEE_VERSION: &str = "0.15.1";
pub const LYCHEE_TOOL_SPEC: &str = concat!(
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

pub fn deny_argv(workspace_roots: &[String]) -> Result<Vec<String>, SupportCommandError> {
    let mut roots = workspace_roots.to_vec();
    roots.sort();
    roots.dedup();
    if roots.is_empty() {
        return Err(SupportCommandError::Contract {
            problem: "deny_requires_workspace".to_owned(),
        });
    }
    for root in &roots {
        super::validate_root(root)?;
    }
    let mut inner = validator_argv(
        "cargo-deny",
        CARGO_DENY_VERSION,
        "cargo",
        &["deny", "--locked", "check"],
    )?;
    if inner.pop().as_deref() != Some("check") {
        return Err(SupportCommandError::Contract {
            problem: "deny_vector_shape_drift".to_owned(),
        });
    }
    let install = IsolatedCommand::mise_install(&[format!("cargo-deny@{CARGO_DENY_VERSION}")])
        .map_err(|err| SupportCommandError::Contract {
            problem: err.to_string(),
        })?;
    let install_argv =
        strings_of(install.argv()).map_err(|problem| SupportCommandError::Contract { problem })?;
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
                super::isolated_manifest_flag(workspace)
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(super::privilege_drop_argv(&install_command, &payload))
}

fn join_quoted_argv(argv: &[String]) -> Result<String, SupportCommandError> {
    argv.iter()
        .map(|element| crate::quote_run_arg(element))
        .collect::<Result<Vec<_>, _>>()
        .map(|words| words.join(" "))
        .map_err(SupportCommandError::from)
}

pub fn machete_argv() -> Result<Vec<String>, SupportCommandError> {
    let mut args = vec!["machete"];
    args.extend(MACHETE_SCAN_CRATES);
    validator_argv(
        CARGO_MACHETE_TOOL_SPEC,
        CARGO_MACHETE_VERSION,
        "cargo",
        &args,
    )
}

pub fn machete_install_argv() -> Result<Vec<String>, SupportCommandError> {
    let install = IsolatedCommand::mise_install(&[format!(
        "{CARGO_MACHETE_TOOL_SPEC}@{CARGO_MACHETE_VERSION}"
    )])
    .map_err(|err| SupportCommandError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(install.argv()).map_err(|problem| SupportCommandError::Contract { problem })
}

pub fn validator_argv(
    spec: &str,
    version: &str,
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, SupportCommandError> {
    validator_argv_parts(spec, version, program, args.iter().copied())
}

pub fn validator_argv_owned(
    spec: &str,
    version: &str,
    program: &str,
    args: &[String],
) -> Result<Vec<String>, SupportCommandError> {
    validator_argv_parts(spec, version, program, args.iter().map(String::as_str))
}

fn validator_argv_parts<'a>(
    spec: &str,
    version: &str,
    program: &'a str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, SupportCommandError> {
    if !VALIDATOR_TOOL_SPECS.contains(&spec) {
        return Err(SupportCommandError::Contract {
            problem: format!("validator_tool_rejected:{spec}"),
        });
    }
    validate_exact_version(spec, version).map_err(|err| SupportCommandError::Contract {
        problem: err.to_string(),
    })?;
    let payload: Vec<OsString> = std::iter::once(program)
        .chain(args)
        .map(OsString::from)
        .collect();
    let exec =
        IsolatedCommand::mise_exec(&[format!("{spec}@{version}")], &payload).map_err(|err| {
            SupportCommandError::Contract {
                problem: err.to_string(),
            }
        })?;
    strings_of(exec.argv()).map_err(|problem| SupportCommandError::Contract { problem })
}

pub(super) fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}
