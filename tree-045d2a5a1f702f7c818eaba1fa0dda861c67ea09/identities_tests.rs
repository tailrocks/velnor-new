//! Group-identity tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::*;

use velnor_actions_contract::cachekey::{
    FormatInputs, cache_format_id, mbx_cache_generation, toolchain_id,
};
use velnor_actions_contract::{digest_b3, validate_digest};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_planning::component_id_for_unit;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::TaskKind;
use velnor_actions_rust_core::{CompileDriver, LocalEdge, TestRunner, WorkspaceRecord};

#[test]
fn lanes_follow_responsibility_never_ordinals() {
    let workspace = digest_b3(b"workspace");
    let clippy = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        CompileDriver::Cargo,
        TestRunner::CargoTest,
    );
    assert_eq!(
        lane_id_for(&clippy, &workspace),
        lane_id_for(&clippy, &workspace)
    );
    assert!(validate_digest(&lane_id_for(&clippy, &workspace)).is_ok());
    let test = group(
        TaskKind::Test,
        "stack/rust/root/test/default",
        CompileDriver::Cargo,
        TestRunner::CargoTest,
    );
    assert_ne!(
        lane_id_for(&clippy, &workspace),
        lane_id_for(&test, &workspace)
    );
    let shard_a = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-1-of-2",
        CompileDriver::Cargo,
        TestRunner::CargoNextest,
    );
    let shard_b = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-2-of-2",
        CompileDriver::Cargo,
        TestRunner::CargoNextest,
    );
    assert_ne!(
        lane_id_for(&shard_a, &workspace),
        lane_id_for(&shard_b, &workspace)
    );
    assert_eq!(writer_lane_for(&clippy.task_id), "primary");
    assert_eq!(writer_lane_for(&shard_a.task_id), "shard-1-of-2");
    for malformed in [
        "stack/rust/root/nextest/default/shard-x",
        "stack/rust/root/nextest/default/shard-0-of-2",
        "stack/rust/root/nextest/default/shard-1-of-2/shard-3-of-4",
        "stack/rust/root/nextest/shard-1-of-2/default",
    ] {
        assert_eq!(writer_lane_for(malformed), "primary", "{malformed}");
    }
}

#[test]
fn toolchains_bind_sorted_specs_driver_runner() {
    let catalog = ToolCatalog::pinned();
    let cargo = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        CompileDriver::Cargo,
        TestRunner::CargoTest,
    );
    let inputs = toolchain_inputs_for(&cargo, &catalog).expect("rust toolchain inputs");
    let mut sorted = inputs.tools.clone();
    sorted.sort();
    assert_eq!(inputs.tools, sorted);
    assert!(inputs.tools.iter().any(|spec| spec.starts_with("rust@")));
    let nextest = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default",
        CompileDriver::Cargo,
        TestRunner::CargoNextest,
    );
    let nextest_inputs = toolchain_inputs_for(&nextest, &catalog).expect("nextest inputs");
    assert!(nextest_inputs.tools.len() > inputs.tools.len());
    let mbx = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        CompileDriver::Mbx,
        TestRunner::CargoTest,
    );
    assert_ne!(
        toolchain_digest_for(&cargo, &catalog).expect("digest"),
        toolchain_digest_for(&mbx, &catalog).expect("digest")
    );
    assert_ne!(
        toolchain_digest_for(&cargo, &catalog).expect("digest"),
        toolchain_digest_for(&nextest, &catalog).expect("digest")
    );
}

#[test]
fn formats_stay_single_and_graphs_relocate() {
    assert_ne!(
        cache_format_id_for(CompileDriver::Cargo),
        cache_format_id_for(CompileDriver::Mbx)
    );
    assert!(validate_digest(&cache_format_id_for(CompileDriver::Cargo)).is_ok());
    assert!(validate_digest(&cache_format_id_for(CompileDriver::Mbx)).is_ok());
    let record = |id: &str| WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![id.to_owned()],
        packages: vec![velnor_actions_rust_core::PackageRecord {
            id: id.to_owned(),
            name: "a".to_owned(),
            version: "0.1.0".to_owned(),
            manifest: "a/Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: Vec::new(),
            has_build_script: false,
        }],
        edges: vec![LocalEdge {
            from: id.to_owned(),
            to: id.to_owned(),
            kind: velnor_actions_rust_core::DepKind::Normal,
            optional: false,
            target: None,
        }],
        skipped_edges: Vec::new(),
    };
    let digest_of = |id: &str| {
        super::super::snapshot::canonical_digest(&snapshot_graph_for(&record(id))).expect("digest")
    };
    assert_eq!(
        digest_of("path+file:///old#a@0.1.0"),
        digest_of("path+file:///new#a@0.1.0")
    );
    assert_eq!(component_id_for_unit("a-id", "a/Cargo.toml"), "a-id");
}

#[test]
fn cache_format_identity_tracks_emitted_mbx_generation() {
    let version = velnor_actions_mise::catalog::MR_BOXINGTON_VERSION;
    let generation = mbx_cache_generation(version);
    let current = cache_format_id(&FormatInputs {
        adapter: "mbx".to_owned(),
        format: "velnor-cache-v1".to_owned(),
        generation,
    })
    .expect("current MBX format identity");
    let stale = cache_format_id(&FormatInputs {
        adapter: "mbx".to_owned(),
        format: "velnor-cache-v1".to_owned(),
        generation: mbx_cache_generation("1.21.0"),
    })
    .expect("stale MBX format identity");

    assert_eq!(cache_format_id_for(CompileDriver::Mbx), current);
    assert_ne!(
        current, stale,
        "a cache generation change must re-key identity"
    );
}

/// Minimal tofu proposal with `kind` and driver spellings.
fn tofu_task(kind: &str) -> ProposedTask {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use velnor_actions_contract_planning::{
        CachePolicy, IdentityInputs, ResourceClass, ResourceDemand,
    };
    ProposedTask {
        task_id: format!("stack/tofu/root/{kind}/default"),
        stack_id: "tofu".to_owned(),
        component_id: "tofu:".to_owned(),
        task_kind: kind.to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: String::new(),
            unit_key: "root".to_owned(),
            unit_path: String::new(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "tofu".to_owned(),
            test_runner: "tofu".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("tofu")],
        display_name: String::new(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

#[test]
fn tofu_toolchain_pins_opentofu_plus_provider_surface() {
    let inputs = toolchain_inputs_for(&tofu_task("validate"), &ToolCatalog::pinned())
        .expect("tofu converts");
    assert_eq!(inputs.tools, vec!["opentofu@1.13.1".to_owned()]);
    assert_eq!(inputs.components.len(), 1);
    let entry = &inputs.components[0];
    assert!(entry.starts_with("tofu-provider-inputs:b3-"), "{entry}");
    let digest = entry.strip_prefix("tofu-provider-inputs:").expect("prefix");
    assert!(validate_digest(digest).is_ok());
    assert_eq!(inputs.compile_driver, "tofu");
    assert_eq!(inputs.test_runner, "tofu");
    assert!(toolchain_id(&inputs).is_ok());
}

#[test]
fn tofu_toolchain_flips_on_provider_surface() {
    let catalog = ToolCatalog::pinned();
    let digest_for = |task: &ProposedTask| toolchain_digest_for(task, &catalog).expect("digest");
    let validate = digest_for(&tofu_task("validate"));
    assert_eq!(validate, digest_for(&tofu_task("init")));
    assert_ne!(validate, digest_for(&tofu_task("fmt")));
    let mut other = tofu_task("validate");
    other.identity.unit_key = "stacks/vpc".to_owned();
    assert_ne!(validate, digest_for(&other));
    assert!(toolchain_inputs_for(&tofu_task("bogus"), &catalog).is_err());
}

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn tofu_cache_format_is_distinct_and_valid() {
    let tofu = cache_format_id_for_tofu();
    assert!(validate_digest(&tofu).is_ok());
    assert_ne!(tofu, cache_format_id_for(CompileDriver::Cargo));
    assert_ne!(tofu, cache_format_id_for(CompileDriver::Mbx));
}

/// Minimal discovery with no workspaces or tool checks.
fn empty_discovery() -> crate::discover::Discovery {
    crate::discover::Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

#[test]
fn tofu_bundles_skip_rust_checkout_probes() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = empty_discovery();
    let snapshot = super::super::snapshot::ExecutionSnapshot::build(&discovery);
    // A nonexistent root proves no probe runs: tofu slots resolve
    // through the tofu bridge, never here.
    let bundle = extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(std::path::Path::new("/nonexistent-tofu-guard")),
        None,
    );
    let inputs = bundle.inputs();
    assert_eq!(
        inputs.lock_digest,
        velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned())
    );
    assert_eq!(
        inputs.nextest_digest,
        velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned())
    );
}

#[test]
fn ordinary_stack_runner_platform_cannot_expand_with_named_check_catalog() {
    let rust = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        CompileDriver::Cargo,
        TestRunner::CargoTest,
    );
    let tofu = tofu_task("validate");
    for task in [rust, tofu] {
        for target in [
            "host",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
        ] {
            let mut task = task.clone();
            task.identity.target = target.to_owned();
            assert!(platform_id_for_group("macos-15", &task).is_err());
            assert!(platform_id_for_group("ephemeral-linux", &task).is_err());
        }
        let mut mac_target = task;
        mac_target.identity.target = "aarch64-apple-darwin".to_owned();
        assert!(platform_id_for_group("ubuntu-24.04", &mac_target).is_err());
    }
}
