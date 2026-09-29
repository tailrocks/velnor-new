//! Evidence-byte collection for execution-profile detection.

use std::path::Path;

use velnor_actions_rust::{
    EvidenceFile, FileIndex, ProfileInputs, WorkspaceRecord, detect_profile,
};
use velnor_actions_workflow_renderer::marker::MARKER_PREFIX;

use crate::OrchestratorError;
use crate::discover::PlannedWorkspace;

/// Detect the execution profile from bytes read under `root`.
///
/// Generated workflow output (marker-prefixed) is never evidence; only
/// durable signals outside `.github` plus handwritten workflows qualify.
///
/// # Errors
///
/// Returns [`OrchestratorError::Profile`] on ambiguous test runners.
pub(crate) fn profile_for_workspace(
    root: &Path,
    index: &FileIndex,
    record: &WorkspaceRecord,
) -> Result<velnor_actions_rust::ProfileOutcome, OrchestratorError> {
    let _ = record;
    let tool_text = read_optional(root, "mise.toml").or(read_optional(root, ".mise.toml"));
    let cargo_a = read_optional(root, ".cargo/config.toml");
    let cargo_b = read_optional(root, ".cargo/config");
    let exec_texts = collect_executables(root, index);
    let flow_texts = collect_handwritten(root, index);
    let mut executables = Vec::with_capacity(exec_texts.len());
    for (path, text) in &exec_texts {
        executables.push(EvidenceFile {
            path,
            content: text,
        });
    }
    let mut flows = Vec::with_capacity(flow_texts.len());
    for (path, text) in &flow_texts {
        flows.push(EvidenceFile {
            path,
            content: text,
        });
    }
    let inputs = ProfileInputs {
        tool_config: tool_text.as_ref().map(|text| EvidenceFile {
            path: "mise.toml",
            content: text,
        }),
        cargo_configs: cargo_files(cargo_a.as_ref(), cargo_b.as_ref()),
        executables,
        handwritten_workflows: flows,
    };
    detect_profile(&inputs).map_err(|err| OrchestratorError::Profile {
        problem: err.to_string(),
    })
}

/// Collect executable task bytes under `.mise/tasks/`.
fn collect_executables(root: &Path, index: &FileIndex) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for path in index.files() {
        if (path == ".mise/tasks" || path.starts_with(".mise/tasks/"))
            && let Some(text) = read_executable(root, path)
        {
            out.push((path.clone(), text));
        }
    }
    out
}

/// Collect handwritten workflow bytes, skipping generated output.
fn collect_handwritten(root: &Path, index: &FileIndex) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for path in index.files() {
        if is_workflow_path(path)
            && let Some(text) = read_optional(root, path)
            && !starts_generated(&text)
        {
            out.push((path.clone(), text));
        }
    }
    out
}

/// Borrow the Cargo config blobs as evidence files.
fn cargo_files<'a>(first: Option<&'a String>, second: Option<&'a String>) -> Vec<EvidenceFile<'a>> {
    let mut files = Vec::new();
    if let Some(text) = first {
        files.push(EvidenceFile {
            path: ".cargo/config.toml",
            content: text,
        });
    }
    if let Some(text) = second {
        files.push(EvidenceFile {
            path: ".cargo/config",
            content: text,
        });
    }
    files
}

/// Read an optional UTF-8 file; missing or unreadable yields `None`.
fn read_optional(root: &Path, relative: &str) -> Option<String> {
    std::fs::read_to_string(root.join(relative)).ok()
}

/// Read an executable file; non-executable or unreadable yields `None`.
fn read_executable(root: &Path, relative: &str) -> Option<String> {
    let path = root.join(relative);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if meta.is_symlink() || meta.is_dir() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    std::fs::read_to_string(&path).ok()
}

/// True for workflow paths considered as handwritten evidence.
fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && path
            .rsplit('.')
            .next()
            .is_some_and(|ext| ext == "yml" || ext == "yaml")
}

/// True when the first line carries the generated marker prefix.
fn starts_generated(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.starts_with(MARKER_PREFIX))
}

/// Committed-profile drift warnings for every workspace (GAP-C.1, VER-4.2).
///
/// Compares the committed generated workflow against freshly detected
/// driver/runner spellings and the configured runner label; drift warns,
/// never fails.
pub(crate) fn workspace_drift_warnings(
    root: &Path,
    workspaces: &[PlannedWorkspace],
    label: &str,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let committed = read_optional(
        root,
        velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
    );
    let Some(bytes) = committed.as_deref() else {
        return warnings;
    };
    for workspace in workspaces {
        let driver = workspace.profile.compile_driver.as_str();
        let runner = workspace.profile.test_runner.as_str();
        let previous = velnor_actions_rust::read_committed_profile_for_comparison(bytes);
        if velnor_actions_rust::committed_profile_differs(previous.as_ref(), driver, runner) {
            let prev = previous
                .as_ref()
                .map(|known| format!("{}/{}", known.compile_driver, known.test_runner));
            warnings.push(format!(
                "committed_profile_drift:{driver}/{runner}:committed_{}:persist a durable signal",
                prev.as_deref().unwrap_or("unknown")
            ));
        }
        if let Some(committed_label) = committed_runs_on(bytes)
            && velnor_actions_contract::runner_family_changed(&committed_label, label)
        {
            warnings.push(format!(
                "runner_family_changed:{committed_label}:{label}:requalify_toolchain"
            ));
        }
    }
    warnings
}

/// First `runs-on` value in committed bytes, if any.
fn committed_runs_on(bytes: &str) -> Option<String> {
    for line in bytes.lines() {
        let Some((_, value)) = line.split_once("runs-on:") else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}
