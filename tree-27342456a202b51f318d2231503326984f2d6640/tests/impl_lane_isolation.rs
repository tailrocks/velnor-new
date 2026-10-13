//! Lane isolation: no active concurrent writers share a target dir.
//!
//! Investigation (kept here so the negative result stays greppable):
//! no generated path runs two Cargo writers against one target dir, so
//! no distinct target-dir wiring exists and none is invented.
//!
//! - Matrix legs run on separate runners with fresh filesystems; the
//!   `strategy.max-parallel` cap limits leg count, never co-locates
//!   writers on one runner.
//! - Steps within one leg run sequentially: V1 emits no
//!   `parallel:`/`background:` step syntax.
//! - Test shards expand into separate matrix entries (separate legs),
//!   never co-located steps — and partitioning stays off by default
//!   (`test_sharding.default_shards = 1`).
//!
//! The lane helpers (`lane_cargo_target_env`, the `cargo_target_dir`
//! plan metadata) therefore have no YAML consumer: the checked-in
//! `ci.yml` carries zero `CARGO_TARGET_DIR`. This test pins that
//! disabled status: the partition gate stays at one shard and the
//! rendered workflow emits no lane target-dir wiring. Wiring distinct
//! target dirs becomes required only if a future path co-locates
//! concurrent Cargo writers on one runner.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

#[test]
fn partitions_stay_off_and_no_lane_wiring_renders() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    assert_eq!(
        prep.config.test_sharding.default_shards, 1,
        "partition gate stays off by default"
    );
    assert!(
        prep.config.test_sharding.by_manifest.is_empty(),
        "no manifest shard overrides by default"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or("missing workflow")?;
    assert!(
        !yaml.contains("CARGO_TARGET_DIR"),
        "no lane target-dir wiring without concurrent writers"
    );
    Ok(())
}
