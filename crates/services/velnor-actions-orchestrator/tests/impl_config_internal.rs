//! Init/config plus internal plan/merge integration cases.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::FinalReport;
use velnor_actions_orchestrator::{
    OrchestratorError, finalized_jobs, init_config, merge_internal, plan_internal, plan_text,
    prepare, resolve_root,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, fixture_manifest_json, git, make_repo, passing_reports,
    plan_for_source_change, without_ambient_identity,
};
use crate::impl_merge::task_reports_for;

#[path = "impl_config_internal/config.rs"]
mod config;
#[path = "impl_config_internal/plan.rs"]
mod plan;
#[path = "impl_config_internal/validation.rs"]
mod validation;

/// Uncomment a sample line when it carries TOML after the `#`.
fn uncomment_sample_line(line: &str) -> &str {
    let Some(rest) = line.strip_prefix('#') else {
        return line;
    };
    let code = rest.trim_start();
    if code.starts_with('[') || code.starts_with('"') || is_assignment(code) {
        code
    } else {
        line
    }
}

/// True for `key = value` sample lines (prose has no assignment).
fn is_assignment(code: &str) -> bool {
    let Some((key, _)) = code.split_once('=') else {
        return false;
    };
    !key.trim().is_empty()
        && key.trim().bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'"' | b'/' | b' ')
        })
        && !key.contains("  ")
}
