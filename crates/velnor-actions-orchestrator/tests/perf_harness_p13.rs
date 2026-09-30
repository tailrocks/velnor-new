//! P13 perf harness: plan timing plus obligation extraction.
//!
//! Included via `#[path]` from `impl_perf_p13`, so the parent wires a
//! single `mod` line for the whole perf suite.

use std::fs;
use std::path::Path;
use std::time::Instant;

use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::plan_internal;

use crate::impl_common::{git, git_line};

/// Wall time of `op` in whole milliseconds plus its value.
pub(crate) fn timed<T>(op: impl FnOnce() -> T) -> (T, u128) {
    let start = Instant::now();
    let value = op();
    (value, start.elapsed().as_millis())
}

/// Commit everything twice, touching `touch_rel` for the second commit.
/// Returns `(base, head)` shas for planning.
pub(crate) fn commit_two(
    root: &Path,
    touch_rel: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let touch = root.join(touch_rel);
    let mut body = fs::read_to_string(&touch)?;
    body.push_str("pub fn g() {}\n");
    fs::write(&touch, body)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    Ok((base, head))
}

/// Plan `head` against `base`; returns the validated plan plus raw JSON.
pub(crate) fn plan_at(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((plan, response))
}

/// Commit twice and plan head; the standard perf-case plan run.
pub(crate) fn plan_two_commits(
    root: &Path,
    touch_rel: &str,
) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let (base, head) = commit_two(root, touch_rel)?;
    plan_at(root, &base, &head)
}

/// Sorted task IDs across every obligation.
pub(crate) fn obligation_task_ids(plan: &Plan) -> Vec<String> {
    let mut ids: Vec<String> = plan.obligations.iter().map(|o| o.task_id.clone()).collect();
    ids.sort();
    ids
}

/// Machine-readable perf line on stderr (visible with `--nocapture`).
pub(crate) fn perf_line(op: &str, crates: usize, wall_ms: u128, plan: &Plan) {
    eprintln!(
        "perf: op={op} crates={crates} wall_ms={wall_ms} obligations={} matrix_entries={}",
        plan.obligations.len(),
        plan.matrix.include.len()
    );
}
