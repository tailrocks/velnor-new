//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::OsString;

use velnor_actions_mise::{
    IsolatedCommand, PinnedTool, PinnedToolExec, ToolCatalog, validate_exact_version,
};
use velnor_actions_rust::TaskGroup;
use velnor_actions_rust::tasks::cargo_payload_argv;
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::OrchestratorError;
use crate::qualify::QualifyRequest;

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

/// Mise tool specs the policy vectors may select, without versions.
const POLICY_TOOL_SPECS: [&str; 2] = ["cargo-deny", "ubi:bnjbvr/cargo-machete"];

/// Product crates scanned by the machete vector, in contract order.
///
/// Fixed paths keep the intentional `fixtures/symlink-escape` negative
/// fixture out of the scan: a bare `cargo machete` walk errors on the
/// fixture's dangling `src` symlink instead of skipping it.
const MACHETE_SCAN_CRATES: [&str; 7] = [
    "crates/velnor-actions-contract",
    "crates/velnor-actions-rust",
    "crates/velnor-actions-mise",
    "crates/velnor-actions-actionlint",
    "crates/velnor-actions-workflow-renderer",
    "crates/velnor-actions-orchestrator",
    "crates/velnor-actions-cli",
];

/// V1 fixed vector for one group: pinned `mise` payload plus kind args.
pub(crate) fn task_argv(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let mut tools = vec![PinnedTool::Rust];
    if group.compile_driver == "mbx" {
        tools.push(PinnedTool::MrBoxington);
    }
    let program = OsString::from("cargo");
    let exec = PinnedToolExec::new(tools, &program, cargo_payload_argv(group)).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: a pinned `gh` version probe.
pub(crate) fn verify_tools_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let program = OsString::from("gh");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        &program,
        vec![OsString::from("--version")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: `cargo deny --locked check` through pinned Mise.
pub(crate) fn deny_argv() -> Result<Vec<String>, OrchestratorError> {
    policy_argv(
        "cargo-deny",
        CARGO_DENY_VERSION,
        "cargo",
        &["deny", "--locked", "check"],
    )
}

/// Fixed policy-job vector: `cargo machete` over product crates via Mise.
pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec!["machete"];
    args.extend(MACHETE_SCAN_CRATES);
    policy_argv(
        "ubi:bnjbvr/cargo-machete",
        CARGO_MACHETE_VERSION,
        "cargo",
        &args,
    )
}

/// One policy vector: an allowlisted tool spec plus a fixed cargo payload.
///
/// Built through the Mise adapter's isolated `exec` constructor, so the
/// emitted shape (global flags, spec, `--` separator, payload) matches the
/// typed `PinnedToolExec` vectors byte for byte. The spec name must be
/// allowlisted and the version an exact pin; anything else fails closed.
fn policy_argv(
    spec: &str,
    version: &str,
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, OrchestratorError> {
    if !POLICY_TOOL_SPECS.contains(&spec) {
        return Err(OrchestratorError::Contract {
            problem: format!("policy_tool_rejected:{spec}"),
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
    let program = OsString::from("mbx");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::MrBoxington],
        &program,
        vec![OsString::from("--version")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed bootstrap §4 build vector through pinned Mise.
///
/// Shared by the candidate build and the pre-seed helper build, so both
/// compile `velnor-actions-cli`/`velnor-actions` with the exact same
/// pinned Rust plus MBX toolchain and flags.
pub(crate) fn candidate_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let mbx = OsString::from("mbx");
    let build = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        &mbx,
        fixed(&[
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
    )
    .map_err(|err| OrchestratorError::Contract {
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

/// Build a fixed argument list.
fn fixed(flags: &[&str]) -> Vec<OsString> {
    flags.iter().map(OsString::from).collect()
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_vectors_pin_specs_and_payloads() {
        let deny = deny_argv().expect("deny argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "cargo-deny@0.20.2",
            "--",
            "cargo",
            "deny",
            "--locked",
            "check",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(deny, want);
        let machete = machete_argv().expect("machete argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "ubi:bnjbvr/cargo-machete@0.9.2",
            "--",
            "cargo",
            "machete",
            "crates/velnor-actions-contract",
            "crates/velnor-actions-rust",
            "crates/velnor-actions-mise",
            "crates/velnor-actions-actionlint",
            "crates/velnor-actions-workflow-renderer",
            "crates/velnor-actions-orchestrator",
            "crates/velnor-actions-cli",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(machete, want);
        assert!(policy_argv("evil-tool", "1.2.3", "cargo", &["deny"]).is_err());
        assert!(policy_argv("cargo-deny", "latest", "cargo", &["deny"]).is_err());
    }

    #[test]
    fn mbx_probe_vector_is_byte_exact() {
        let probe = mbx_probe_argv(&ToolCatalog::pinned()).expect("probe argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "mr-boxington@1.19.0",
            "--",
            "mbx",
            "--version",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(probe, want);
    }

    #[test]
    fn section4_build_vector_is_byte_exact() {
        let build = candidate_build_argv(&ToolCatalog::pinned()).expect("build argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(build, want);
    }
}
