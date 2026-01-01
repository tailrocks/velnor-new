//! Remediation cases: PAR audit rows.
use crate::impl_shared_fixtures::{MANIFEST, TASK, sample_identity};
use crate::impl_shared_fixtures::{sample_plan, valid_config};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ArchiveInputs, ContractError, archive_id, artifact_id_for_baseline, digest_b3, input_digest,
    run_key_for_ci, validate_artifact_id,
};
use velnor_actions_contract_config::{ShardTimingEvidence, validate_shard_changes_need_evidence};
use velnor_actions_contract_planning::graph::{CpuMilli, MemoryMb};
use velnor_actions_contract_planning::{
    CachePolicy, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph, TaskNode,
};
use velnor_actions_contract_workflow::ManifestTaskProof;

#[test]
fn par_ignore_sorted_dup_unknown() {
    let mut config = valid_config();
    assert_eq!(config.validate("cfg"), Ok(()));
    config.stacks.ignore = vec!["bogus".to_owned(), "rust".to_owned()];
    let err = config.validate("cfg").expect_err("unknown stack");
    assert!(err.to_string().contains("unknown_stack_id:bogus"));
    config.stacks.ignore = vec!["rust".to_owned(), "rust".to_owned()];
    let err = config.validate("cfg").expect_err("duplicate stack");
    assert!(err.to_string().contains("duplicate_stack_id"));
}

#[test]
fn par_task_node_carries_thirteen_fields() {
    let node = sample_node("stack/rust/root/clippy/default").expect("node");
    assert_eq!(node.validate(), Ok(()));
    let keys: Vec<String> = serde_json::to_value(&node)
        .expect("value")
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        [
            "cache_policy",
            "component_id",
            "configuration",
            "depends_on",
            "gated_by",
            "input_digest",
            "lane_id",
            "outputs",
            "reads",
            "resource",
            "stack_id",
            "task_id",
            "task_kind",
            "writes",
        ]
    );
    let mut bad = node.clone();
    bad.stack_id = "bogus".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = node;
    bad.depends_on = vec!["zzz".to_owned(), "aaa".to_owned()];
    assert!(bad.validate().is_err());
}

/// Sample graph node shared by PAR cases.
fn sample_node(task_id: &str) -> Result<TaskNode, ContractError> {
    Ok(TaskNode {
        task_id: task_id.to_owned(),
        stack_id: "rust".to_owned(),
        component_id: "demo 0.1.0".to_owned(),
        task_kind: "clippy".to_owned(),
        configuration: "default".to_owned(),
        input_digest: digest_b3(b"inputs"),
        depends_on: vec![],
        gated_by: vec![],
        reads: vec!["src/lib.rs".to_owned()],
        writes: vec![],
        outputs: vec!["target/report.json".to_owned()],
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: Some(CpuMilli::new(2000)?),
            memory_mb: Some(MemoryMb::new(4096)?),
            needs_network: false,
            service: None,
        },
        lane_id: digest_b3(b"lane"),
        cache_policy: CachePolicy {
            allow_compilation_reuse: true,
            allow_task_reuse: true,
        },
    })
}

#[test]
fn par_edges_distinguish_four_kinds() {
    let clippy = "stack/rust/root/clippy/default";
    let test = "stack/rust/root/test/default";
    let mut clippy_node = sample_node(clippy).expect("node");
    clippy_node.outputs = vec!["target/clippy.json".to_owned()];
    let mut test_node = sample_node(test).expect("node");
    test_node.task_kind = "test".to_owned();
    test_node.depends_on = vec![clippy.to_owned()];
    test_node.gated_by = vec![clippy.to_owned()];
    let graph = TaskGraph {
        nodes: vec![clippy_node, test_node],
        edges: vec![
            TaskEdge {
                from: clippy.to_owned(),
                to: test.to_owned(),
                kind: EdgeKind::Data,
            },
            TaskEdge {
                from: clippy.to_owned(),
                to: test.to_owned(),
                kind: EdgeKind::Gate,
            },
            TaskEdge {
                from: clippy.to_owned(),
                to: test.to_owned(),
                kind: EdgeKind::Report,
            },
            TaskEdge {
                from: clippy.to_owned(),
                to: test.to_owned(),
                kind: EdgeKind::ResourceExclusion,
            },
        ],
    };
    assert_eq!(graph.validate(), Ok(()));
    let kinds: Vec<EdgeKind> = graph.edges.iter().map(|edge| edge.kind).collect();
    assert_eq!(
        kinds,
        [
            EdgeKind::Data,
            EdgeKind::Gate,
            EdgeKind::Report,
            EdgeKind::ResourceExclusion
        ]
    );
    let mut bad = graph.clone();
    bad.edges.push(TaskEdge {
        from: clippy.to_owned(),
        to: clippy.to_owned(),
        kind: EdgeKind::Data,
    });
    assert!(bad.validate().is_err());
    let mut dangling = graph;
    dangling.nodes.pop();
    assert!(dangling.validate().is_err());
}

#[test]
fn par_git_revision_change_disables_reuse() -> Result<(), ContractError> {
    let clean = input_digest(&sample_identity())?;
    let mut observed = sample_identity();
    observed.vcs.commit = Some("ab".repeat(20));
    observed.vcs.reference = Some("refs/heads/main".to_owned());
    observed.vcs.submodules = BTreeMap::from([("vendor/dep".to_owned(), digest_b3(b"dep"))]);
    let revised = input_digest(&observed)?;
    assert_ne!(revised, clean);
    let mut moved = observed.clone();
    moved.vcs.commit = Some("cd".repeat(20));
    assert_ne!(input_digest(&moved)?, revised);
    assert_eq!(input_digest(&observed)?, revised);
    let mut bad = sample_identity();
    bad.vcs.commit = Some("short".to_owned());
    assert!(bad.validate().is_err());
    let mut bad = sample_identity();
    bad.vcs.reference = Some("has space".to_owned());
    assert!(bad.validate().is_err());
    Ok(())
}

#[test]
fn par_task_ids_match_obligations() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(31, 1);
    let mut plan = sample_plan(&run_key)?;
    plan.validate()?;
    plan.task_ids = vec!["stack/rust/root/test/default".to_owned()];
    let err = plan.validate().expect_err("mismatch");
    assert_eq!(
        err,
        ContractError::identity("task_ids", "obligation_mismatch")
    );
    let mut plan = sample_plan(&run_key)?;
    let mut extra = plan.obligations[0].clone();
    extra.task_id = "stack/rust/root/test/default".to_owned();
    plan.obligations.push(extra);
    plan.obligations.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    assert!(plan.validate().is_err());
    Ok(())
}

#[test]
fn par_baseline_artifact_name_shape() -> Result<(), ContractError> {
    let commit = "ab".repeat(20);
    let compat = digest_b3(b"compat");
    let name = artifact_id_for_baseline(&commit, &compat)?;
    assert_eq!(name, format!("velnor-baseline-{commit}-{compat}"));
    assert_eq!(validate_artifact_id(&name), Ok(()));
    assert!(artifact_id_for_baseline("short", &compat).is_err());
    assert!(artifact_id_for_baseline(&"AB".repeat(20), &compat).is_err());
    assert!(artifact_id_for_baseline(&commit, "nope").is_err());
    assert!(validate_artifact_id("velnor-baseline-short-b3-zzz").is_err());
    Ok(())
}

#[test]
fn par_resource_classes_cover_six_kinds() {
    let classes = [
        ResourceClass::Lightweight,
        ResourceClass::Network,
        ResourceClass::Compiler,
        ResourceClass::Test,
        ResourceClass::Service,
        ResourceClass::Exclusive,
    ];
    assert_eq!(classes.len(), 6);
    for class in classes {
        let demand = ResourceDemand {
            class,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        };
        assert_eq!(demand.validate(), Ok(()), "{class:?}");
    }
    let bad = ResourceDemand {
        class: ResourceClass::Test,
        cpu_milli: None,
        memory_mb: None,
        needs_network: false,
        service: Some("db".to_owned()),
    };
    assert!(bad.validate().is_err());
    assert!(CpuMilli::new(0).is_err());
    assert!(MemoryMb::new(0).is_err());
    assert_eq!(CpuMilli::new(2000).expect("cpu").get(), 2000);
}

#[test]
fn par_archive_identity_covers_eleven_inputs() -> Result<(), ContractError> {
    let inputs = ArchiveInputs {
        source_digest: digest_b3(b"source"),
        package: "demo".to_owned(),
        target: "host".to_owned(),
        features: vec!["default".to_owned()],
        profile: "test".to_owned(),
        toolchain_id: digest_b3(b"toolchain"),
        runtime: "glibc-2.39".to_owned(),
        test_runner: "cargo-nextest@0.9.96".to_owned(),
        format: "tar.zst".to_owned(),
        platform_id: digest_b3(b"platform"),
        config_digest: digest_b3(b"config"),
    };
    let keys: Vec<String> = serde_json::to_value(&inputs)
        .expect("value")
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys.len(), 11);
    let id = archive_id(&inputs)?;
    assert_eq!(id, archive_id(&inputs)?);
    let mut changed = inputs.clone();
    changed.test_runner = "cargo-nextest@0.9.97".to_owned();
    assert_ne!(archive_id(&changed)?, id);
    let mut platform = inputs.clone();
    platform.platform_id = digest_b3(b"other-platform");
    assert_ne!(archive_id(&platform)?, id);
    let mut config = inputs.clone();
    config.config_digest = digest_b3(b"other-config");
    assert_ne!(archive_id(&config)?, id);
    let mut unsorted = inputs.clone();
    unsorted.features = vec!["b".to_owned(), "a".to_owned()];
    assert!(archive_id(&unsorted).is_err());
    let mut bad = inputs.clone();
    bad.toolchain_id = "nope".to_owned();
    assert!(archive_id(&bad).is_err());
    let mut bad = inputs;
    bad.platform_id = "nope".to_owned();
    assert!(archive_id(&bad).is_err());
    Ok(())
}

#[test]
fn par_shards_capped_by_test_budget() {
    let mut config = valid_config();
    assert_eq!(config.validate("cfg"), Ok(()));
    config.test_sharding.by_manifest = BTreeMap::from([("crates/large/Cargo.toml".to_owned(), 4)]);
    let err = config.validate("cfg").expect_err("over budget");
    assert!(err.to_string().contains("exceeds_test_process_budget"));
    let mut config = valid_config();
    config.test_sharding.default_shards = 9;
    assert!(config.validate("cfg").is_err());
    let mut config = valid_config();
    config.test_sharding.by_manifest = BTreeMap::from([("crates/large/Cargo.toml".to_owned(), 2)]);
    assert_eq!(config.validate("cfg"), Ok(()));
}

#[test]
fn par_shard_changes_require_timing_evidence() {
    let previous = valid_config().test_sharding;
    let mut next = previous.clone();
    next.default_shards = 2;
    let err = validate_shard_changes_need_evidence(&previous, &next, &[], "cfg")
        .expect_err("default change needs evidence");
    assert!(
        err.to_string()
            .contains("shard_change_without_timing_evidence")
    );
    let default_evidence = ShardTimingEvidence {
        manifest: None,
        timing_digest: digest_b3(b"timing"),
    };
    assert_eq!(
        validate_shard_changes_need_evidence(&previous, &next, &[default_evidence], "cfg"),
        Ok(())
    );
    let mut scoped = previous.clone();
    scoped.by_manifest = BTreeMap::from([(MANIFEST.to_owned(), 2)]);
    let err = validate_shard_changes_need_evidence(&previous, &scoped, &[], "cfg")
        .expect_err("manifest change needs evidence");
    assert!(
        err.to_string()
            .contains("shard_change_without_timing_evidence")
    );
    let scoped_evidence = ShardTimingEvidence {
        manifest: Some(MANIFEST.to_owned()),
        timing_digest: digest_b3(b"timing"),
    };
    assert_eq!(
        validate_shard_changes_need_evidence(&previous, &scoped, &[scoped_evidence], "cfg"),
        Ok(())
    );
    let mut bad = previous.clone();
    bad.default_shards = 2;
    let wrong_scope = ShardTimingEvidence {
        manifest: Some(MANIFEST.to_owned()),
        timing_digest: digest_b3(b"timing"),
    };
    assert!(validate_shard_changes_need_evidence(&previous, &bad, &[wrong_scope], "cfg").is_err());
    assert_eq!(
        validate_shard_changes_need_evidence(&previous, &previous, &[], "cfg"),
        Ok(())
    );
}

#[test]
fn par_manifest_task_proof_binds_identities() {
    let task = digest_b3(b"task");
    let inputs = digest_b3(b"inputs");
    let graph = digest_b3(b"graph");
    let toolchain = digest_b3(b"toolchain");
    let mbx = digest_b3(b"mbx");
    let platform = digest_b3(b"platform");
    let build = |task_id: &str, digest: &str, run: u64| {
        ManifestTaskProof::new(
            task_id, digest, &inputs, &graph, &toolchain, &mbx, &platform, "test", run,
        )
    };
    let proof = build(TASK, &task, 4242).expect("valid proof");
    assert_eq!(proof.validate(), Ok(()));
    assert_eq!(proof.task_id(), TASK);
    assert_eq!(proof.proof_run_id(), 4242);
    assert!(build(TASK, "nope", 4242).is_err());
    assert!(build(TASK, &task, 0).is_err());
    assert!(build("bogus", &task, 4242).is_err());
}
