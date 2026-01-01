//! F2 scheduling tests: lanes, partitions, timing, ownership, identity.

use velnor_actions_orchestrator::decisions::preview_unique_dir;
use velnor_actions_orchestrator::schedule::{
    TaskTiming, aggregate_timings, assign_lanes, cache_ownership_table, distribute_by_weight,
    effective_weight, fanout_worthwhile, overlap_ratio, partition_count, resource_exclusions,
    sequential_reference,
};

use crate::impl_common::TestResult;

#[test]
fn lanes_isolate_cargo_writers() {
    use velnor_actions_workflow_renderer::steps::{TARGET_DIR_PREFIX, target_dir_for_lane};
    let ids = ["b".to_owned(), "a".to_owned(), "c".to_owned()];
    let lanes = assign_lanes(&ids);
    assert_eq!(lanes.len(), 3);
    let mut seen = std::collections::BTreeSet::new();
    for (_, lane) in &lanes {
        assert!(seen.insert(*lane), "distinct lanes");
    }
    assert_eq!(lanes[0].0, "a");
    assert_eq!(
        assign_lanes(&ids),
        assign_lanes(&["c".to_owned(), "b".to_owned(), "a".to_owned()])
    );
    let dirs: Vec<String> = lanes
        .iter()
        .map(|(_, lane)| target_dir_for_lane(&lane.to_string()))
        .collect();
    assert!(dirs.iter().all(|dir| dir.starts_with(TARGET_DIR_PREFIX)));
    let unique: std::collections::BTreeSet<&str> = dirs.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), 3, "distinct target dirs");
}

#[test]
fn shared_lanes_get_exclusions() {
    let pairs = resource_exclusions(&[("a", 0), ("b", 0), ("c", 1)]);
    assert_eq!(pairs, [("a".to_owned(), "b".to_owned())]);
    assert!(resource_exclusions(&[("a", 0), ("b", 1)]).is_empty());
}

#[test]
fn partitions_cover_every_test_exactly_once() {
    assert_eq!(effective_weight(Some(120)), 120);
    assert_eq!(effective_weight(Some(0)), 1);
    assert_eq!(effective_weight(None), 1);
    assert_eq!(partition_count(0, 4), 1);
    assert_eq!(partition_count(1, 4), 1);
    assert_eq!(partition_count(3, 4), 1);
    assert_eq!(partition_count(100, 4), 4);
    let weights = [10, 1, 1, 1, 1, 1];
    let shards = distribute_by_weight(&weights, 2);
    let mut covered: Vec<usize> = shards.iter().flatten().copied().collect();
    covered.sort_unstable();
    assert_eq!(covered, [0, 1, 2, 3, 4, 5]);
    assert_eq!(shards.len(), 2);
}

#[test]
fn fanout_needs_measured_savings() {
    assert!(fanout_needs(100, 10, 10));
    assert!(!fanout_needs(20, 10, 10));
    assert!(!fanout_needs(19, 10, 10));
    assert!(!fanout_needs(0, 0, 0));
    assert!(fanout_needs(u64::MAX, u64::MAX - 1, 0));
}

/// Local alias keeping the gate name visible at the call site.
fn fanout_needs(saving: u64, setup: u64, transfer: u64) -> bool {
    fanout_worthwhile(saving, setup, transfer)
}

#[test]
fn sequential_reference_is_sorted_set() {
    let ids = ["b".to_owned(), "a".to_owned(), "c".to_owned()];
    assert_eq!(sequential_reference(&ids), ["a", "b", "c"]);
    assert!(sequential_reference(&[]).is_empty());
}

#[test]
fn timings_measure_slots_without_double_count() {
    let timing = TaskTiming {
        queue_ms: 1,
        runner_ms: 2,
        task_ms: 3,
        cache_ms: 4,
        prep_ms: 5,
        download_ms: 6,
        compiler_ms: 7,
        mbx_ms: 8,
        test_ms: 9,
        lock_wait_ms: 10,
    };
    assert_eq!(timing.accounted_total(), 55);
    assert_eq!(TaskTiming::exclusive_ms(100, 30), 70);
    assert_eq!(TaskTiming::exclusive_ms(10, 30), 0);
    let total = aggregate_timings(&[timing.clone(), timing]);
    assert_eq!(total.queue_ms, 2);
    assert_eq!(total.lock_wait_ms, 20);
    assert_eq!(total.accounted_total(), 110);
}

#[test]
fn overlap_ratio_stays_descriptive() {
    for spans in [&[][..], &[(0, 10)][..], &[(0, 10), (10, 20)][..]] {
        let ratio = overlap_ratio(spans);
        assert!(ratio.abs() < f64::EPSILON, "{spans:?} -> {ratio}");
    }
    let half = overlap_ratio(&[(0, 10), (0, 10)]);
    assert!((half - 0.5).abs() < f64::EPSILON, "{half}");
    let partial = overlap_ratio(&[(0, 10), (5, 15)]);
    assert!((partial - 0.25).abs() < f64::EPSILON, "{partial}");
}

#[test]
fn cache_paths_have_single_owners() {
    use velnor_actions_workflow_renderer::steps::{
        TARGET_DIR_PREFIX, TASK_ARTIFACTS_DIR, TOOLS_CACHE_PATH,
    };
    let table = cache_ownership_table();
    let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for (path, owner) in &table {
        assert!(seen.insert(path, owner).is_none(), "duplicate path {path}");
    }
    assert!(table.len() >= 5, "all five layers owned");
    for (index, (left, _)) in table.iter().enumerate() {
        for (other, _) in table.iter().skip(index + 1) {
            assert!(
                !left.starts_with(other) && !other.starts_with(left),
                "{left} vs {other}"
            );
        }
    }
    let paths: Vec<&str> = table.iter().map(|(path, _)| *path).collect();
    assert!(paths.contains(&TARGET_DIR_PREFIX), "target lane owner");
    assert!(paths.contains(&TOOLS_CACHE_PATH), "tool owner");
    assert!(paths.contains(&TASK_ARTIFACTS_DIR), "task-result owner");
    assert!(
        !paths.iter().any(|path| path.contains("candidate")),
        "candidate artifacts are run-scoped, never cache-owned"
    );
}

#[test]
fn preview_dirs_are_unique_tmp_roots() {
    let first = preview_unique_dir("velnor-preview");
    let second = preview_unique_dir("velnor-preview");
    assert_ne!(first, second);
    for dir in [&first, &second] {
        assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
        assert!(!dir.exists(), "absent until written");
    }
}

#[test]
fn failed_tasks_never_save_results() {
    use velnor_actions_mise::cache::save_allowed;
    for event in ["push", "pull_request", "merge_group"] {
        assert!(!save_allowed("trusted", event, false), "{event}");
    }
    assert!(save_allowed("trusted", "push", true));
    assert!(!save_allowed("trusted", "pull_request", true));
    assert!(!save_allowed("trusted", "merge_group", true));
}

#[test]
fn reuse_qualification_rejects_nondeterminism() {
    use velnor_actions_mise::cache::qualify_reuse;
    for kind in ["publish", "deploy", "notify", "service"] {
        assert!(qualify_reuse(kind, false, false, false).is_err(), "{kind}");
    }
    for flags in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert!(
            qualify_reuse("test", flags.0, flags.1, flags.2).is_err(),
            "{flags:?}"
        );
    }
    assert!(qualify_reuse("test", false, false, false).is_ok());
    assert!(qualify_reuse("clippy", false, false, false).is_ok());
}

#[test]
fn lookup_failures_are_distinct_from_current() {
    use velnor_actions_contract::FreshnessStatus;
    let failed = serde_json::to_value(FreshnessStatus::LookupFailed).expect("json");
    let current = serde_json::to_value(FreshnessStatus::Current).expect("json");
    assert_eq!(
        failed,
        serde_json::Value::String("lookup_failed".to_owned())
    );
    assert_ne!(failed, current);
    assert_ne!(FreshnessStatus::LookupFailed, FreshnessStatus::Stale);
}

/// Minimal task proposal for identity-extension cases.
fn clippy_task() -> Result<velnor_actions_contract::ProposedTask, Box<dyn std::error::Error>> {
    let group = velnor_actions_rust::TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: velnor_actions_rust::TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group)?;
    task.validate()?;
    Ok(task)
}

/// Workspace digests for identity-extension cases.
fn extension_inputs<'a>(
    graph: &'a str,
    config: &'a str,
    lock: velnor_actions_rust::tasks::DigestSlot,
    targets: &'a [String],
) -> velnor_actions_rust::GroupExtensionInputs<'a> {
    velnor_actions_rust::GroupExtensionInputs {
        package_id: "demo",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: graph,
        targets,
        config_digest: config,
        lock_digest: lock,
        nextest_digest: velnor_actions_rust::tasks::DigestSlot::Unknown("unprobed".to_owned()),
        archive_source: None,
        rerun_inputs: None,
        has_build_script: false,
    }
}

#[test]
fn identity_extension_carries_no_tool_inputs() -> TestResult {
    let task = clippy_task()?;
    let targets = Vec::new();
    let graph = velnor_actions_contract::digest_b3(b"graph");
    let config = velnor_actions_contract::digest_b3(b"config");
    let inputs = extension_inputs(
        &graph,
        &config,
        velnor_actions_rust::tasks::DigestSlot::Unknown("unprobed".to_owned()),
        &targets,
    );
    let data = serde_json::to_value(
        velnor_actions_rust::extension_for_proposal(&task, &inputs)?
            .to_stack_extension()
            .data,
    )?;
    let keys: std::collections::BTreeSet<String> = data
        .as_object()
        .ok_or("extension object")?
        .keys()
        .cloned()
        .collect();
    for key in &keys {
        assert!(
            !key.contains("tool") && !key.contains("mise") && !key.contains("toolchain"),
            "tool input in identity: {key}"
        );
    }
    assert!(keys.contains("lock_digest"), "lock stays a task input");
    Ok(())
}

#[test]
fn platform_image_changes_invalidate_identity() -> TestResult {
    use velnor_actions_contract::cachekey::{PlatformInputs, platform_id};
    let inputs = |image_version: &str| PlatformInputs {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        image_os: "ubuntu26".to_owned(),
        image_version: image_version.to_owned(),
        target: "host".to_owned(),
    };
    assert_eq!(
        platform_id(&inputs("20260101.1"))?,
        platform_id(&inputs("20260101.1"))?
    );
    assert_ne!(
        platform_id(&inputs("20260101.1"))?,
        platform_id(&inputs("20260201.1"))?
    );
    Ok(())
}

#[test]
fn extension_identity_tracks_lock_digest() -> TestResult {
    use velnor_actions_rust::tasks::DigestSlot;
    let task = clippy_task()?;
    let targets = Vec::new();
    let graph = velnor_actions_contract::digest_b3(b"graph");
    let config = velnor_actions_contract::digest_b3(b"config");
    let lock_a = velnor_actions_contract::digest_b3(b"lock-a");
    let lock_b = velnor_actions_contract::digest_b3(b"lock-b");
    let json = |lock: DigestSlot| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let inputs = extension_inputs(&graph, &config, lock, &targets);
        Ok(serde_json::to_value(
            velnor_actions_rust::extension_for_proposal(&task, &inputs)?
                .to_stack_extension()
                .data,
        )?)
    };
    let known_a = || DigestSlot::Known(lock_a.clone());
    assert_eq!(json(known_a())?, json(known_a())?);
    assert_ne!(json(known_a())?, json(DigestSlot::Known(lock_b.clone()))?);
    assert_ne!(
        json(known_a())?,
        json(DigestSlot::Unknown("unprobed".to_owned()))?
    );
    Ok(())
}
