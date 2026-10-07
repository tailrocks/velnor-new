//! F2 structural scans: spawning, paths, registries, suggestions.

use std::path::{Path, PathBuf};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::merge_status;

#[path = "impl_orch_runtime_acquisition.rs"]
mod runtime_acquisition;

/// Orchestrator `src/` directory.
pub(crate) fn orch_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Family `src/` directories: the hub plus every extracted sibling crate.
///
/// Structural scans span the family so moved modules stay covered.
/// Membership is the `velnor-actions-orchestrator` prefix (the hub
/// itself plus every `orchestrator-` sibling), so later extractions
/// join the scan without touching this file.
fn family_src_dirs() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(services) = manifest.parent() else {
        return Ok(vec![manifest.join("src")]);
    };
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(services)? {
        let member = entry?.path();
        let name = member
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name == "velnor-actions-orchestrator" || name.starts_with("velnor-actions-orchestrator-")
        {
            dirs.push(member.join("src"));
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Sorted `.rs` files directly under every family `src/`.
pub(crate) fn src_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for dir in family_src_dirs()? {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
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

/// Every `path:line` holding `token` in orchestrator family code.
fn token_hits(token: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut hits = Vec::new();
    for path in src_files()? {
        for (line, code) in code_of(&path)? {
            if code.contains(token) {
                hits.push(format!("{}:{line}", path.display()));
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
        let (path, line) = hit.rsplit_once(':').unwrap_or(("", ""));
        let line: usize = line.parse().unwrap_or(0);
        let code = code_of(Path::new(path))?;
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
    for path in src_files()? {
        // Test companions assert wrapper shape; they never ship wrappers.
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs"))
        {
            continue;
        }
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
    use velnor_actions_contract_config::VelnorConfig;
    use velnor_actions_orchestrator::{DetectorInfo, detector_registry};
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
    for path in src_files()? {
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
    for path in src_files()? {
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

/// `TaskStatus` variants in declaration order.
fn task_status_variants() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../core/velnor-actions-contract-workflow/src/workflow/report.rs");
    let text = std::fs::read_to_string(&path)?;
    let mut variants = Vec::new();
    let mut in_enum = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "pub enum TaskStatus {" {
            in_enum = true;
            continue;
        }
        if in_enum {
            if trimmed == "}" {
                break;
            }
            if let Some(name) = trimmed.strip_suffix(',').filter(|name| {
                name.chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
                    && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
            }) {
                variants.push(name.to_owned());
            }
        }
    }
    Ok(variants)
}

#[test]
fn reference_copies_neither_omissions_nor_retries() -> TestResult {
    for token in [
        "retry_pass",
        "first_attempt",
        "slow_test",
        "excluded_slow",
        "skip_slow",
        "omitted_test",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    assert_eq!(
        task_status_variants()?,
        [
            "Reused",
            "Executed",
            "EmptyPartition",
            "NotSelected",
            "Failed",
            "Cancelled"
        ]
    );
    Ok(())
}

#[test]
fn nonzero_retries_rejected_while_retry_state_absent() -> TestResult {
    assert!(
        !task_status_variants()?
            .iter()
            .any(|name| name.contains("Retry")),
        "no retry-passed state while retries are disabled"
    );
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let extra = serde_json::json!({"limits": {"compiler_budget": 2, "test_budget": 2, "max_parallel": 2, "capacity": 8, "shards": 1, "retries": 1}});
    assert_eq!(
        merge_status(&plan, &reports, &[], &extra)?,
        velnor_actions_contract_workflow::FinalStatus::PlanningFailed
    );
    Ok(())
}

#[test]
fn merge_consumes_no_publish_verbs() -> TestResult {
    for token in [
        "release_upload",
        "publish_baseline",
        "upload_baseline",
        "put_object",
        "gh api",
        "workflows/upload",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    let baseline = std::fs::read_to_string(orch_src().join("cover_baseline.rs"))?;
    assert!(baseline.contains("no_publish_attempted"), "PR guard stays");
    Ok(())
}

#[test]
fn offline_deps_fail_closed_without_fetch() -> TestResult {
    for path in src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (line, code) in code_of(&path)? {
            if name == "source_prep.rs" {
                assert!(
                    !code.contains(".run("),
                    "source_prep emits fetch steps, never executes: {}:{line}",
                    path.display()
                );
                continue;
            }
            let scrubbed = runtime_acquisition::scrub_bound_acquisition(&name, &code)
                .replace("fetch_inventory", "")
                .replace("FetchFailure", "")
                .replace("fetch_add", "")
                // Gate-1 emission threading: plan-job `cargo fetch` step
                // builders plus their lockful-root inputs. The argv literal
                // lives in source_prep.rs (scoped above, execution-free);
                // analysis-time fetching stays forbidden.
                .replace("fetch_steps", "")
                .replace("fetch_roots", "")
                // Plan checkout input emission: `fetch-depth: 0` is a
                // workflow input literal (history for HEAD^2 + base diff),
                // never an analysis-time fetch execution.
                .replace("fetch-depth", "")
                // Contract error-code literal asserted by cache-key
                // rejection tests, never an execution.
                .replace("unsafe_fetch_root", "");
            assert!(
                !scrubbed.contains("fetch"),
                "fetch verb at {}:{line}: {code}",
                path.display()
            );
        }
    }
    Ok(())
}
