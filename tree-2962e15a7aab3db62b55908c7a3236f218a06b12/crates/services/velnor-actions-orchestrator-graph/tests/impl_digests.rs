//! Canonical digests and task-identity binding.

use velnor_actions_orchestrator_graph::internal_plan::snapshot::{
    canonical_digest, parse_canonical_json,
};
use velnor_actions_orchestrator_graph::internal_plan::task_digest::task_digest;

#[test]
fn task_digest_is_deterministic() {
    let argv = ["mise".to_owned(), "verify".to_owned()];
    let first = task_digest("task-1", &argv, "toolchain-1").expect("digest");
    let second = task_digest("task-1", &argv, "toolchain-1").expect("digest");
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn task_digest_binds_argv() {
    let task = "task-1";
    let toolchain = "toolchain-1";
    let base = task_digest(task, &["mise".to_owned()], toolchain).expect("digest");
    let changed =
        task_digest(task, &["mise".to_owned(), "x".to_owned()], toolchain).expect("digest");
    assert_ne!(base, changed);
}

#[test]
fn task_digest_binds_toolchain() {
    let argv = ["mise".to_owned()];
    let base = task_digest("task-1", &argv, "toolchain-1").expect("digest");
    let changed = task_digest("task-1", &argv, "toolchain-2").expect("digest");
    assert_ne!(base, changed);
}

#[test]
fn task_digest_binds_task_id() {
    let argv = ["mise".to_owned()];
    let base = task_digest("task-1", &argv, "toolchain-1").expect("digest");
    let changed = task_digest("task-2", &argv, "toolchain-1").expect("digest");
    assert_ne!(base, changed);
}

#[test]
fn canonical_digest_is_stable_and_sensitive() {
    let first = canonical_digest(&("a", 1)).expect("digest");
    let second = canonical_digest(&("a", 1)).expect("digest");
    let other = canonical_digest(&("a", 2)).expect("digest");
    assert!(!first.is_empty());
    assert_eq!(first, second);
    assert_ne!(first, other);
}

#[test]
fn canonical_json_rejects_duplicate_keys() {
    let value = parse_canonical_json("{\"a\":1}").expect("parses");
    assert_eq!(value["a"], 1);
    parse_canonical_json("{\"a\":1,\"a\":2}").expect_err("dup keys rejected");
}
