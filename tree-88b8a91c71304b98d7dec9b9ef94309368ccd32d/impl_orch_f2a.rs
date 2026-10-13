//! F2 structural scans: spawning, paths, registries, suggestions.

use std::path::{Path, PathBuf};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::merge_status;

/// Orchestrator `src/` directory.
fn orch_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Sorted `.rs` files directly under `src/`.
fn src_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(orch_src())? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
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
fn code_of(path: &Path) -> Result<Vec<(usize, String)>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect())
}

/// Every `name:line` holding `token` in orchestrator code.
fn token_hits(token: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut hits = Vec::new();
    for path in src_files()? {
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
    let mut sh_files = std::collections::BTreeSet::new();
    for path in src_files()? {
        let text = std::fs::read_to_string(&path)?;
        if text.contains("\"sh\"") {
            sh_files.insert(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert_eq!(
        sh_files,
        std::collections::BTreeSet::from([
            "pins.rs".to_owned(),
            "qualify.rs".to_owned(),
            "workflow_jobs.rs".to_owned(),
        ]),
        "fixed sh wrappers live in pins/qualify/workflow_jobs only"
    );
    Ok(())
}

#[test]
fn plan_and_generate_share_one_prepare_path() -> TestResult {
    let mut discover_calls = Vec::new();
    let mut config_calls = Vec::new();
    for path in src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (line, code) in code_of(&path)? {
            if code.contains("discover(") && !code.contains("fn discover(") {
                discover_calls.push(format!("{name}:{line}"));
            }
            if code.contains("load_config(") && !code.contains("fn load_config(") {
                config_calls.push(format!("{name}:{line}"));
            }
        }
    }
    assert_eq!(discover_calls.len(), 1, "{discover_calls:?}");
    assert!(
        discover_calls[0].starts_with("prepare.rs"),
        "{discover_calls:?}"
    );
    assert_eq!(config_calls.len(), 1, "{config_calls:?}");
    assert!(
        config_calls[0].starts_with("prepare.rs"),
        "{config_calls:?}"
    );
    let internal = std::fs::read_to_string(orch_src().join("internal.rs"))?;
    assert!(internal.contains("prepare(&root)"), "plan runs prepare");
    let generate = std::fs::read_to_string(orch_src().join("generate.rs"))?;
    assert!(
        generate.contains("prep: &GenerationPreparation"),
        "generate consumes preparation"
    );
    Ok(())
}

#[test]
fn v1_registers_rust_only() {
    use velnor_actions_contract::VelnorConfig;
    use velnor_actions_orchestrator::decisions::{DetectorInfo, detector_registry};
    assert_eq!(VelnorConfig::REGISTERED_STACKS, &["rust"]);
    assert_eq!(
        detector_registry(),
        vec![DetectorInfo {
            stack_id: "rust",
            schema: 1
        }]
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
        .join("../velnor-actions-contract/src/workflow/report.rs");
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
        velnor_actions_contract::FinalStatus::PlanningFailed
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

/// All `.rs` files under a crate-relative directory, recursively.
fn tree_rs(relative: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    let mut out = Vec::new();
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn cli_carries_no_stack_flags() -> TestResult {
    for path in tree_rs("../velnor-actions-cli/src")? {
        let text = std::fs::read_to_string(&path)?;
        assert!(
            !text.to_lowercase().contains("stack"),
            "stack flag in {}",
            path.display()
        );
    }
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
            let scrubbed = code
                .replace("fetch_inventory", "")
                .replace("FetchFailure", "")
                .replace("fetch_add", "")
                // Gate-1 emission threading: plan-job `cargo fetch` step
                // builders plus their lockful-root inputs. The argv literal
                // lives in source_prep.rs (scoped above, execution-free);
                // analysis-time fetching stays forbidden.
                .replace("fetch_steps", "")
                .replace("fetch_roots", "");
            assert!(
                !scrubbed.contains("fetch"),
                "fetch verb at {}:{line}: {code}",
                path.display()
            );
        }
    }
    Ok(())
}
