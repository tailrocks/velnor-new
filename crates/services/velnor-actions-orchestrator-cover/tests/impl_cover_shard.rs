use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::digest_b3;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_cover::cover::shard::{
    BaselineLookup, ResourceLimits, ShardProof, check_entry_shards, resolve_manifests,
    validate_budgets,
};

/// In-budget limits with ordered resource groups.
fn limits() -> ResourceLimits {
    ResourceLimits {
        compiler_budget: 2,
        test_budget: 4,
        max_parallel: 2,
        capacity: 8,
        shards: 2,
        retries: 0,
        resource_groups: vec!["a".to_owned(), "b".to_owned()],
    }
}

#[test]
fn budgets_reject_every_overage() {
    let mut zero = limits();
    zero.compiler_budget = 0;
    assert_eq!(
        validate_budgets(&zero).expect_err("zero budget"),
        "budget_must_be_positive"
    );
    let mut retries = limits();
    retries.retries = 1;
    assert_eq!(
        validate_budgets(&retries).expect_err("retries"),
        "retries_disabled"
    );
    let mut shards = limits();
    shards.shards = 9;
    assert_eq!(
        validate_budgets(&shards).expect_err("shards"),
        "shards_exceed_test_budget"
    );
    let mut parallel = limits();
    parallel.max_parallel = 9;
    assert_eq!(
        validate_budgets(&parallel).expect_err("parallel"),
        "concurrency_above_capacity"
    );
    let mut unordered = limits();
    unordered.resource_groups = vec!["b".to_owned(), "a".to_owned()];
    assert_eq!(
        validate_budgets(&unordered).expect_err("unordered"),
        "resource_groups_unordered"
    );
    let mut blank = limits();
    blank.resource_groups = vec!["".to_owned()];
    assert_eq!(
        validate_budgets(&blank).expect_err("blank"),
        "resource_groups_unordered"
    );
}

#[test]
fn budgets_accept_in_budget_limits() {
    assert!(validate_budgets(&limits()).is_ok());
}

#[test]
fn lookup_argv_pins_repo_and_validates() {
    let base = "a".repeat(40);
    let lookup =
        BaselineLookup::new(&base, ".github/workflows/ci.yml", "testmain", "o/r").expect("valid");
    let list: Vec<String> = lookup
        .list_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(list[0..5], ["run", "list", "--repo", "o/r", "--workflow"]);
    let artifacts: Vec<String> = lookup
        .artifacts_args(7)
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(
        artifacts
            .iter()
            .any(|arg| arg == "repos/o/r/actions/runs/7/artifacts")
    );
    assert!(BaselineLookup::new("short", "w", "b", "o/r").is_err());
    assert!(BaselineLookup::new(&base, "w", "b", "not-a-slug").is_err());
}

#[test]
fn resolve_manifests_misses_before_spawning() {
    let catalog = ToolCatalog::pinned();
    let checkout = tempfile::tempdir().expect("tempdir");
    let base = "a".repeat(40);
    assert_eq!(
        resolve_manifests(
            &catalog,
            checkout.path(),
            &base,
            ".github/workflows/ci.yml",
            "testmain",
            None,
            Some("o/r"),
        )
        .expect_err("no artifact"),
        "baseline_no_exact_artifact"
    );
    assert_eq!(
        resolve_manifests(
            &catalog,
            checkout.path(),
            "short",
            ".github/workflows/ci.yml",
            "testmain",
            Some("velnor-baseline-x"),
            Some("o/r"),
        )
        .expect_err("short base"),
        "base_must_be_full_sha"
    );
}

/// Shard proof shell over one task identity.
fn proof(task_id: &str, digest: &str, index: u32, count: u32) -> ShardProof {
    ShardProof {
        task_id: task_id.to_owned(),
        input_digest: digest.to_owned(),
        runner: "cargo_nextest".to_owned(),
        shard_index: index,
        shard_count: count,
        tests: Vec::new(),
        inventory_digest: digest.to_owned(),
        archive_digest: Some("archive".to_owned()),
        no_test_targets: false,
    }
}

#[test]
fn missing_shard_proof_fails() {
    let bases = BTreeSet::from(["stack/rust/demo/clippy/default".to_owned()]);
    let obligations = BTreeMap::new();
    assert_eq!(
        check_entry_shards(&bases, 0, &[], &obligations).expect_err("missing"),
        "missing_shard"
    );
}

#[test]
fn shard_group_mismatch_rejects() {
    let digest = digest_b3(b"shard-inputs");
    let first = "stack/rust/demo/clippy/default/shard-1-of-2".to_owned();
    let second = "stack/rust/demo/clippy/default/shard-2-of-2".to_owned();
    let bases = BTreeSet::from(["stack/rust/demo/clippy/default".to_owned()]);
    let obligations = BTreeMap::from([
        (first.clone(), digest.clone()),
        (second.clone(), digest.clone()),
    ]);
    let proofs = vec![proof(&first, &digest, 1, 2), proof(&second, &digest, 2, 3)];
    assert_eq!(
        check_entry_shards(&bases, 0, &proofs, &obligations).expect_err("mismatch"),
        "shard_group_mismatch"
    );
}
