//! F2 structural scans: spawning, paths, registries, suggestions.

use std::path::{Path, PathBuf};

use crate::impl_common::TestResult;

#[path = "impl_orch_runtime_acquisition.rs"]
pub(crate) mod runtime_acquisition;

/// Orchestrator `src/` directory.
pub(crate) fn orch_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Sorted production `.rs` files directly under `src/`.
///
/// Unit-test companion modules are `#[cfg(test)]`; their assertions and
/// diagnostics must not be mistaken for executable product behavior.
pub(crate) fn product_src_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(orch_src())? {
        let path = entry?.path();
        let test_companion = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs") || name.ends_with("_fixtures.rs"));
        if path.extension().is_some_and(|ext| ext == "rs") && !test_companion {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

/// Code lines of one file: `(number, code)` with comments stripped.
pub(crate) fn code_of(path: &Path) -> Result<Vec<(usize, String)>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect())
}

/// Every `name:line` holding `token` in orchestrator code.
pub(crate) fn token_hits(token: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut hits = Vec::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (line, code) in code_of(&path)? {
            if code.contains(token) {
                hits.push(format!("{name}:{line}"));
            }
        }
    }
    Ok(hits)
}

#[test]
fn orch_spawns_no_processes_and_confines_shell_wrappers() -> TestResult {
    for token in [
        "Command::new",
        "process::Command",
        ".spawn(",
        ".output(",
        "CommandExt",
        "tokio::process",
        "StdCommand",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token} spawns: {:?}",
            token_hits(token)?
        );
    }
    for hit in token_hits("std::process")? {
        let (name, line) = hit.split_once(':').unwrap_or(("", ""));
        let line: usize = line.parse().unwrap_or(0);
        let code = code_of(&orch_src().join(name))?;
        let body = code
            .iter()
            .find(|(number, _)| *number == line)
            .map(|(_, body)| body.clone())
            .unwrap_or_default();
        assert!(
            body.contains("std::process::id()") && !body.contains("Command"),
            "non-spawn std::process use only: {hit}: {body}"
        );
    }
    // P08 C4: source_prep.rs emits one fixed `sh -c` probe-and-fetch template
    // (metadata probe, offline skip, explicit miss fetch) over validated roots.
    // Step IR has no conditions, so the shell conditional is required; roots
    // are fail-closed validated, never arbitrary shell. P05 reports:
    // matrix_step.rs emits the fixed obligation report-capture wrappers
    // (`sh -c` over a joined validated argv plus the fixed helper call);
    // exit capture needs one shell step, and the joined argv plus helper
    // path are fixed generator values, never repository shell.
    let mut sh_files = std::collections::BTreeSet::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if matches!(
            name.as_str(),
            "validate_shell_yaml.rs" | "validate_shell_yaml_shell.rs"
        ) {
            let text = std::fs::read_to_string(&path)?;
            assert!(!text.contains("StepKind"));
            assert!(!text.contains("argv"));
            assert!(!text.contains("Vec<String>"));
            assert!(!text.contains("Command::"));
            assert!(!text.contains("std::process"));
            assert!(!text.contains("\"-c\""));
            assert!(!text.contains("\"-s\""));
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        if text.contains("\"sh\"") {
            sh_files.insert(name);
        }
    }
    assert_eq!(
        sh_files,
        std::collections::BTreeSet::from([
            "matrix_step.rs".to_owned(),
            "pins.rs".to_owned(),
            "qualify.rs".to_owned(),
            "source_prep.rs".to_owned(),
        ]),
        "fixed sh wrappers live in matrix_step/pins/qualify/source_prep only"
    );
    Ok(())
}

#[path = "impl_orch_prepare.rs"]
mod prepare;

#[test]
fn v1_registers_three_stacks_and_detects_rust_and_tofu() {
    use velnor_actions_contract::VelnorConfig;
    use velnor_actions_orchestrator::decisions::{DetectorInfo, detector_registry};
    assert_eq!(VelnorConfig::REGISTERED_STACKS, &["mise", "rust", "tofu"]);
    assert_eq!(
        detector_registry(),
        vec![
            DetectorInfo {
                stack_id: "rust",
                schema: 1
            },
            DetectorInfo {
                stack_id: "tofu",
                schema: 1
            },
        ]
    );
}

#[test]
fn v1_creates_no_generated_task_dirs() -> TestResult {
    let mut holders = std::collections::BTreeSet::new();
    for path in product_src_files()? {
        let text = std::fs::read_to_string(&path)?;
        if text.contains(".mise/tasks") {
            holders.insert(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert_eq!(
        holders,
        std::collections::BTreeSet::from(["evidence.rs".to_owned()])
    );
    for path in product_src_files()? {
        for (line, code) in code_of(&path)? {
            if code.contains("create_dir") {
                assert!(
                    !code.contains(".mise") && !code.contains("tasks"),
                    "task-dir creation at {}:{line}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn suggestions_are_never_executed() -> TestResult {
    for token in [
        "apply_suggestion",
        "write_suggestion",
        "execute_suggestion",
        "run_suggestion",
        "suggestion_argv",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    Ok(())
}
