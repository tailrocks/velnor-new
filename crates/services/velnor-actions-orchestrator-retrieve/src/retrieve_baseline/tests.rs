//! Fetch gating, argv, branch parsing, and staging tests (no spawns).
//!
//! Every retrieval test misses before any `gh` spawn: unparsable
//! plans, missing bases, execute-all sets, staged evidence, symlink
//! plants, underivable names, and unscoped repos all return `false`
//! without reaching the lookup. The spawn itself stays thin glue
//! over the exact argv and strict parsing tested here.

use super::*;
use velnor_actions_mise::ToolCatalog;

/// Fetch attempt over one plan value in a fresh run directory.
fn attempt(plan: &serde_json::Value, repo: &str) -> bool {
    let tmp = tempfile::tempdir().expect("tempdir");
    retrieve_baseline_to(&ToolCatalog::pinned(), tmp.path(), plan, repo)
}

mod retrieve_baseline_tests;
