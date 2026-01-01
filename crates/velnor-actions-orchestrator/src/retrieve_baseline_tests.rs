//! Fetch gating, argv, branch parsing, and staging tests (no spawns).
//!
//! Every retrieval test misses before any `gh` spawn: unparsable
//! plans, missing bases, execute-all sets, staged evidence, symlink
//! plants, underivable names, and unscoped repos all return `false`
//! without reaching the lookup. The spawn itself stays thin glue
//! over the exact argv and strict parsing tested here.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use velnor_actions_contract::ObligationDecision;
use velnor_actions_mise::ToolCatalog;

/// Covered plan value over `base` (strict round-trip like `plan.json`).
fn covered_plan_value(base: Option<&str>) -> serde_json::Value {
    let manifest = manifest_for(&"1".repeat(40));
    let plan = plan_for(&manifest, base);
    serde_json::to_value(&plan).expect("plan value")
}

/// Fetch attempt over one plan value in a fresh run directory.
fn attempt(plan: &serde_json::Value, repo: &str) -> bool {
    let tmp = tempfile::tempdir().expect("tempdir");
    retrieve_baseline_to(&ToolCatalog::pinned(), tmp.path(), plan, repo)
}

#[test]
fn baseline_skips_unparsable_plan_without_spawning() {
    let plan = serde_json::json!({"schema": 1, "nope": true});
    assert!(!attempt(&plan, "o/r"));
}

#[test]
fn baseline_skips_plan_without_base() {
    assert!(!attempt(&covered_plan_value(None), "o/r"));
}

#[test]
fn baseline_skips_when_nothing_covered() {
    let manifest = manifest_for(&"1".repeat(40));
    let mut plan = plan_for(&manifest, Some(&"1".repeat(40)));
    for obligation in &mut plan.obligations {
        obligation.decision = ObligationDecision::Execute;
        obligation.baseline_proof = None;
    }
    let value = serde_json::to_value(&plan).expect("plan value");
    assert!(!attempt(&value, "o/r"));
}

#[test]
fn baseline_skips_when_staged_file_wins() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let staged = tmp.path().join(crate::baseline_publish::BASELINE_FILENAME);
    std::fs::write(&staged, r#"{"staged":true}"#).expect("staged");
    let plan = covered_plan_value(Some("zz"));
    assert!(!retrieve_baseline_to(
        &ToolCatalog::pinned(),
        tmp.path(),
        &plan,
        "o/r"
    ));
    assert_eq!(
        std::fs::read_to_string(&staged).expect("reread"),
        r#"{"staged":true}"#
    );
}

#[test]
fn baseline_skips_invalid_base_before_branch_lookup() {
    let plan = covered_plan_value(Some("zz"));
    assert!(!attempt(&plan, "o/r"));
}

#[test]
fn baseline_skips_bad_repo_before_branch_lookup() {
    let plan = covered_plan_value(Some(&"1".repeat(40)));
    assert!(!attempt(&plan, "not a slug!!"));
}

#[test]
fn default_branch_args_pins_repo_and_rejects_bad_slug() {
    let args = default_branch_args("o/r");
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(text, ["api", "repos/o/r", "--jq", ".default_branch"]);
    assert!(default_branch_args("not a slug!!").is_empty());
    assert!(default_branch_args("").is_empty());
}

#[test]
fn parse_default_branch_accepts_only_strict_names() {
    assert_eq!(parse_default_branch("main\n").as_deref(), Some("main"));
    assert_eq!(parse_default_branch("\"main\"\n").as_deref(), Some("main"));
    assert_eq!(
        parse_default_branch("feature/x").as_deref(),
        Some("feature/x")
    );
    for bad in [
        "",
        "   \n",
        "HEAD",
        "a b",
        "a\tb",
        "../x",
        "x/../y",
        "a*b",
        "a$b",
        "a;b",
        "x://y",
        "a\"b",
        "\"unterminated",
    ] {
        assert_eq!(parse_default_branch(bad), None, "rejects {bad:?}");
    }
}

#[test]
fn stage_manifest_writes_canonically_and_never_overwrites() {
    let manifest = manifest_for(&"1".repeat(40));
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(stage_manifest(tmp.path(), &manifest));
    let staged = tmp.path().join(crate::baseline_publish::BASELINE_FILENAME);
    let bytes = std::fs::read(&staged).expect("read");
    assert_eq!(bytes, canonical_json_bytes(&manifest).expect("canonical"));
    assert!(!stage_manifest(tmp.path(), &manifest));
    assert_eq!(std::fs::read(&staged).expect("reread"), bytes);
}

#[cfg(unix)]
#[test]
fn baseline_skips_when_staged_symlink_planted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target.json");
    std::fs::write(&target, "{}").expect("target");
    std::os::unix::fs::symlink(
        &target,
        tmp.path().join(crate::baseline_publish::BASELINE_FILENAME),
    )
    .expect("link");
    let plan = covered_plan_value(Some("zz"));
    assert!(!retrieve_baseline_to(
        &ToolCatalog::pinned(),
        tmp.path(),
        &plan,
        "o/r"
    ));
    assert!(!stage_manifest(tmp.path(), &manifest_for(&"1".repeat(40))));
}
