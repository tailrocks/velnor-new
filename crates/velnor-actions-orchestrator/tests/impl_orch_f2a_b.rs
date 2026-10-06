//! F2 follow-up scans: reuse tokens, retries, publish verbs, offline deps.
//!
//! Split from `impl_orch_f2a.rs` by the 400-line repo-size gate.

use std::path::PathBuf;

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_f2a::{code_of, orch_src, product_src_files, runtime_acquisition, token_hits};
use crate::impl_orch_plansel::merge_status;

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

#[test]
fn offline_deps_fail_closed_without_fetch() -> TestResult {
    let source_files = product_src_files()?;
    assert!(
        source_files
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "vectors.rs")),
        "production vectors remain under the offline/fetch scan"
    );
    assert!(
        source_files
            .iter()
            .all(|path| path.file_name().is_none_or(|name| {
                !name.to_string_lossy().ends_with("_tests.rs")
                    && !name.to_string_lossy().ends_with("_fixtures.rs")
            })),
        "test-only companions must not enter product scans"
    );
    for path in source_files {
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
