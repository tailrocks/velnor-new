//! Crate-job ID assignment per stack prefix (T19 G1).
use std::collections::BTreeSet;
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, TOFU_JOB_ID_PREFIX, assign_crate_job_ids, digest_b3, is_crate_job_id,
    slugify_segment, validate_job_id,
};

/// One assignment input triple.
fn triple(id: &str, name: &str, config: &str) -> (String, String, String) {
    (id.to_owned(), name.to_owned(), config.to_owned())
}

/// Tofu groups take `tofu-<slug>`, deterministically and validly.
#[test]
fn tofu_groups_take_the_tofu_prefix() {
    let inputs: BTreeSet<(String, String, String)> = [
        triple("stacks/a", "stacks/a", "default"),
        triple("stacks/b", "stacks/b", "default"),
    ]
    .into_iter()
    .collect();
    let first = assign_crate_job_ids(&inputs, TOFU_JOB_ID_PREFIX);
    let second = assign_crate_job_ids(&inputs, TOFU_JOB_ID_PREFIX);
    assert_eq!(first, second, "assignment is deterministic");
    assert_eq!(
        first.get(&("stacks/a".to_owned(), "default".to_owned())),
        Some(&"tofu-stacks-a".to_owned())
    );
    assert_eq!(
        first.get(&("stacks/b".to_owned(), "default".to_owned())),
        Some(&"tofu-stacks-b".to_owned())
    );
    for id in first.values() {
        assert!(validate_job_id(id).is_ok(), "{id}");
        assert!(is_crate_job_id(id), "{id}");
    }
}

/// Empty slugs fall back per prefix, never bare.
#[test]
fn workspace_fallback_keeps_its_prefix() {
    let inputs: BTreeSet<(String, String, String)> =
        [triple("root", ".", "default")].into_iter().collect();
    assert_eq!(slugify_segment("."), "");
    let tofu = assign_crate_job_ids(&inputs, TOFU_JOB_ID_PREFIX);
    assert_eq!(
        tofu.get(&("root".to_owned(), "default".to_owned())),
        Some(&"tofu-workspace".to_owned())
    );
    let rust = assign_crate_job_ids(&inputs, CRATE_JOB_ID_PREFIX);
    assert_eq!(
        rust.get(&("root".to_owned(), "default".to_owned())),
        Some(&"rust-workspace".to_owned())
    );
}

/// Same slug under both prefixes never collides.
#[test]
fn cross_stack_slugs_never_collide() {
    let inputs: BTreeSet<(String, String, String)> =
        [triple("demo", "demo", "default")].into_iter().collect();
    let rust = assign_crate_job_ids(&inputs, CRATE_JOB_ID_PREFIX);
    let tofu = assign_crate_job_ids(&inputs, TOFU_JOB_ID_PREFIX);
    assert_eq!(
        rust.get(&("demo".to_owned(), "default".to_owned())),
        Some(&"rust-demo".to_owned())
    );
    assert_eq!(
        tofu.get(&("demo".to_owned(), "default".to_owned())),
        Some(&"tofu-demo".to_owned())
    );
}

/// Tofu slug collisions disambiguate within the tofu namespace.
#[test]
fn tofu_collisions_disambiguate_within_prefix() {
    let inputs: BTreeSet<(String, String, String)> = [
        triple("a/b", "a/b", "default"),
        triple("a-b", "a-b", "default"),
    ]
    .into_iter()
    .collect();
    assert_eq!(slugify_segment("a/b"), slugify_segment("a-b"));
    let assigned = assign_crate_job_ids(&inputs, TOFU_JOB_ID_PREFIX);
    assert_eq!(assigned.len(), 2);
    let ids: BTreeSet<&String> = assigned.values().collect();
    assert_eq!(ids.len(), 2, "colliding slugs stay distinct");
    assert_eq!(
        assigned.get(&("a-b".to_owned(), "default".to_owned())),
        Some(&"tofu-a-b".to_owned()),
        "first key in sorted order keeps the base slug"
    );
    let digest = digest_b3("a/b\0default".as_bytes());
    let want = format!("tofu-a-b-{}", &digest[..8]);
    assert_eq!(
        assigned.get(&("a/b".to_owned(), "default".to_owned())),
        Some(&want),
        "later colliding key takes package digest disambiguation"
    );
}

/// The branding gate skips both crate prefixes, nothing else.
#[test]
fn branding_gate_skips_both_crate_prefixes() {
    assert!(validate_job_id("tofu-velnor-infra").is_ok());
    assert!(validate_job_id("rust-velnor-actions-contract").is_ok());
    assert!(validate_job_id("velnor-plan").is_err());
    assert!(validate_job_id("task-velnor-final").is_err());
    assert!(is_crate_job_id("tofu-stacks-a"));
    assert!(is_crate_job_id("rust-demo"));
    for id in ["plan", "required", "actionlint", "candidate"] {
        assert!(!is_crate_job_id(id), "{id}");
    }
}
