//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::OsString;

use velnor_actions_mise::{
    IsolatedCommand, PinnedTool, PinnedToolExec, ToolCatalog, validate_exact_version,
};
use velnor_actions_rust::TaskGroup;
use velnor_actions_rust::tasks::cargo_payload_argv;
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::OrchestratorError;

/// Staged path of the downloaded candidate binary under runner temp.
pub(crate) const CANDIDATE_BINARY_PATH: &str = "$RUNNER_TEMP/velnor/candidate/velnor-actions";

/// Directory receiving the artifact-executed generation check.
pub(crate) const QUALIFY_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/qualify-check";

/// Payload fragments that prove a rebuild; qualify argv must avoid them all.
const REBUILD_MARKERS: [&str; 5] = ["cargo", "mbx", "rustc", "mise", "build"];

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

/// Typed candidate-qualification request: artifact execution, never a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QualifyRequest {
    /// Downloaded candidate binary path (staged by download-artifact).
    binary: String,
    /// Scratch directory for the artifact-executed generation check.
    output_dir: String,
}

impl QualifyRequest {
    /// Fixed request over the staged candidate path.
    pub(crate) fn staged() -> Self {
        Self {
            binary: CANDIDATE_BINARY_PATH.to_owned(),
            output_dir: QUALIFY_OUTPUT_DIR.to_owned(),
        }
    }

    /// Fixed qualify argv: artifact `plan` plus artifact `generate --check`.
    ///
    /// Runs the downloaded binary twice (plan, then a generation check
    /// into scratch) without invoking any build tool. Rejects rebuilds.
    pub(crate) fn argv(&self) -> Result<Vec<String>, OrchestratorError> {
        let script = format!(
            "{} plan && {} generate --output-dir {} && test -f {}/.github/workflows/velnor.yml",
            self.binary, self.binary, self.output_dir, self.output_dir
        );
        let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
        Self::check_no_rebuild(&argv)?;
        Ok(argv)
    }

    /// Reject any qualify vector that could rebuild the candidate.
    pub(crate) fn check_no_rebuild(argv: &[String]) -> Result<(), OrchestratorError> {
        for arg in argv {
            let lower = arg.to_lowercase();
            if REBUILD_MARKERS.iter().any(|mark| lower.contains(mark)) {
                return Err(OrchestratorError::Contract {
                    problem: format!("qualify_must_not_rebuild:{arg}"),
                });
            }
        }
        Ok(())
    }
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

/// Fixed policy-job vector: `cargo machete` through pinned Mise.
pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    policy_argv(
        "ubi:bnjbvr/cargo-machete",
        CARGO_MACHETE_VERSION,
        "cargo",
        &["machete"],
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
    fn qualify_runs_artifact_without_rebuild_markers() {
        let argv = QualifyRequest::staged()
            .argv()
            .map_err(|err| err.to_string());
        assert!(argv.as_ref().is_ok_and(|argv| argv[0] == "sh"));
        assert!(argv.is_ok_and(|argv| {
            argv[2].contains("plan")
                && argv[2].contains("generate")
                && QualifyRequest::check_no_rebuild(&argv).is_ok()
        }));
    }

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
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(machete, want);
        assert!(policy_argv("evil-tool", "1.2.3", "cargo", &["deny"]).is_err());
        assert!(policy_argv("cargo-deny", "latest", "cargo", &["deny"]).is_err());
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

    #[test]
    fn qualify_rebuild_attempt_rejected() {
        for bad in [
            "cargo test",
            "mbx build",
            "rustc x",
            "mise exec",
            "rebuild all",
        ] {
            let err = QualifyRequest::check_no_rebuild(&["sh".to_owned(), bad.to_owned()]);
            assert!(
                err.is_err_and(|err| err.to_string().contains("must_not_rebuild")),
                "{bad}"
            );
        }
    }
}
