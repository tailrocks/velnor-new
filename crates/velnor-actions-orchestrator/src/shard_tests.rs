//! Baseline lookup and shard-proof selection tests.
//!
//! Declared via `#[path]` from `shard.rs` under `cfg(test)` so the
//! lookup module keeps the file size gate.

use super::*;
use crate::run_select::select_exact_base_run;
use velnor_actions_mise::ToolCatalog;

#[test]
fn lookup_args_are_fixed_and_validated() {
    let base = "a".repeat(40);
    let lookup =
        BaselineLookup::new(&base, ".github/workflows/ci.yml", "testmain", "o/r").expect("valid");
    let list: Vec<String> = lookup
        .list_args()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(list[0..5], ["run", "list", "--repo", "o/r", "--workflow"]);
    assert!(BaselineLookup::new("short", "w", "b", "o/r").is_err());
    assert!(BaselineLookup::new(&base, "https://evil/x", "b", "o/r").is_err());
    assert!(BaselineLookup::new(&base, "w", "b", "not-a-slug").is_err());
    assert_eq!(
        BaselineLookup::new(&base, "w", "b", "not-a-slug").expect_err("repo"),
        "bad_lookup_repo".to_owned()
    );
    let other = "b".repeat(40);
    let runs = serde_json::json!([
        {"databaseId": 1, "headSha": other, "headBranch": "t", "event": "push", "conclusion": "success", "attempt": 1},
        {"databaseId": 2, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success", "attempt": 2},
    ]);
    assert_eq!(
        select_exact_base_run(&runs.to_string(), &base, "t"),
        Ok(crate::run_select::SelectedBaseRun {
            run_id: 2,
            attempt: 2
        })
    );
    assert!(select_exact_base_run(&runs.to_string(), &"c".repeat(40), "t").is_err());
    let args: Vec<String> = lookup
        .artifacts_args(7)
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["api", "repos/o/r/actions/runs/7/artifacts"]);
}

#[test]
fn lookup_without_exact_artifact_misses_before_spawning() {
    let base = "a".repeat(40);
    let catalog = ToolCatalog::pinned();
    let tmp = tempfile::tempdir().expect("tempdir");
    let miss = |artifact: Option<&str>| {
        resolve_manifests(
            &catalog,
            tmp.path(),
            &base,
            ".github/workflows/ci.yml",
            "testmain",
            artifact,
            None,
        )
        .expect_err("miss")
    };
    assert_eq!(miss(None), "baseline_no_exact_artifact");
    assert_eq!(miss(Some("")), "baseline_no_exact_artifact");
    let malformed = resolve_manifests(
        &catalog,
        tmp.path(),
        "short",
        ".github/workflows/ci.yml",
        "testmain",
        None,
        None,
    )
    .expect_err("inputs");
    assert_eq!(malformed, "base_must_be_full_sha");
}
