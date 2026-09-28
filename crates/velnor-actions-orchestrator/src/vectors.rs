//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::OsString;

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
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

/// Fixed candidate build plus qualification vectors.
pub(crate) fn candidate_spec(catalog: &ToolCatalog) -> Result<CandidateSpec, OrchestratorError> {
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
    let qualify = QualifyRequest::staged().argv()?;
    Ok(CandidateSpec {
        build: strings_of(build.argv(catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?,
        qualify,
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
