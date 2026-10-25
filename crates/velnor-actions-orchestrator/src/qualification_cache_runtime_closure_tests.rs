//! Post-task source identity checks for Rust, `ToFu`, and named checks.

use super::{
    ObservedLaneClosure, PostTaskClosureContext, recompute_post_task_closure, require_plan_match,
};
use crate::discover::Discovery;
use crate::internal::plan_obligation::{self, identity::TaskIdentityDerivationInputs};
use crate::internal_plan::identities::{platform_id_for_group, toolchain_inputs_for};
use crate::internal_plan::snapshot::{ExecutionSnapshot, platform_target_for_group};
use crate::internal_plan::{adapter_metadata, execute_ids};
use crate::vectors::task_argv;
use tempfile::TempDir;
use velnor_actions_contract::{
    CheckExecutor, CheckPlatform, CheckRunner, MatrixEntry, MiseCheck, ObligationDecision,
    PlanGenerator, PlanObligation, PlannedPlatform, ProposedTask,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

const RUN_KEY: &str = "local";
const RUST_RUNNER: &str = "ubuntu-26.04";

fn generator() -> PlanGenerator {
    PlanGenerator {
        version: "0.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn discovery_with_proposals(proposals: Vec<ProposedTask>) -> Discovery {
    Discovery {
        affected: crate::affected_inputs::AffectedInputs::default(),
        gradle_roots: Vec::new(),
        mise_checks: Vec::new(),
        native_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
        bun: crate::workflow_task_jobs::bun_inventory::BunInventory::default(),
        dbt: crate::workflow_task_jobs::dbt_inventory::DbtInventory::default(),
        ansible: crate::workflow_task_jobs::ansible_inventory::AnsibleInventory::default(),
    }
}

fn rust_task() -> ProposedTask {
    let task = velnor_actions_rust::propose_task(&TaskGroup {
        all_features: false,
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo 0.1.0".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    })
    .expect("Rust proposal");
    task.validate().expect("valid task");
    task
}

fn tofu_task() -> ProposedTask {
    let task = velnor_actions_tofu::propose_task(&velnor_actions_tofu::TofuTaskGroup {
        root: String::new(),
        kind: velnor_actions_tofu::TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    })
    .expect("ToFu proposal");
    task.validate().expect("valid task");
    task
}

fn write_rust_source(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).expect("source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("manifest");
    std::fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").expect("source");
}

fn planned_adapter(
    root: &std::path::Path,
    discovery: &Discovery,
    task: &ProposedTask,
    job_id: &str,
    runs_on: &str,
) -> (MatrixEntry, PlanObligation) {
    let catalog = ToolCatalog::pinned();
    let generator = generator();
    let toolchain_inputs = toolchain_inputs_for(task, &catalog).expect("toolchain inputs");
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)
        .expect("toolchain identity");
    let argv = task_argv(task, &catalog).expect("task command");
    let platform_id = platform_id_for_group(RUST_RUNNER, task).expect("platform identity");
    let snapshot = ExecutionSnapshot::build(discovery);
    let identity = plan_obligation::identity::derive_task_identity(
        &TaskIdentityDerivationInputs {
            root,
            discovery,
            task,
            argv: &argv,
            toolchain: &toolchain,
            platform_id: &platform_id,
            generator: &generator,
            snapshot: &snapshot,
        },
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect("planned identity");
    let task_digest =
        plan_obligation::task_digest(&task.task_id, &argv, &toolchain).expect("task digest");
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv).expect("run command");
    let metadata = adapter_metadata(task, &[]).expect("adapter metadata");
    let target = platform_target_for_group(RUST_RUNNER, task).expect("target");
    let entry = MatrixEntry::derive(
        &task.stack_id,
        &task.task_id,
        &run,
        &task_digest,
        metadata,
        execute_ids(task),
        &identity.input_digest,
        RUN_KEY,
        job_id,
        PlannedPlatform::new(runs_on, &target).expect("planned platform"),
    )
    .expect("matrix entry");
    let mut entry = entry;
    entry.toolchain_inputs = Some(toolchain_inputs);
    entry.toolchain_id = Some(toolchain);
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "test".to_owned(),
        task_digest: task_digest.clone(),
        task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(task_digest)
            .expect("binding digest"),
        input_digest: identity.input_digest.clone(),
        closure_digest: identity.closure_digest.clone(),
        graph_digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        mbx_digest: "b3-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        profile: "default".to_owned(),
        job_id: entry.job_id.clone(),
    };
    (entry, obligation)
}

fn check_discovery(root: &std::path::Path, check: &MiseCheck) -> (Discovery, DiscoveredCheck) {
    let item = velnor_actions_mise::discover_checks(root, std::slice::from_ref(check), &[])
        .expect("named-check discovery")
        .into_iter()
        .next()
        .expect("one check");
    let mut found = discovery_with_proposals(vec![item.proposal.clone()]);
    found.mise_checks.push(item.clone());
    (found, item)
}

fn observed_rust(
    root: &std::path::Path,
    discovery: &Discovery,
    entry: &MatrixEntry,
) -> ObservedLaneClosure {
    observed_for(root, discovery, RUST_RUNNER, &entry.matrix_key, entry).expect("post-task closure")
}

fn observed_for(
    root: &std::path::Path,
    discovery: &Discovery,
    logical_runner_label: &str,
    matrix_key: &str,
    entry: &MatrixEntry,
) -> Result<ObservedLaneClosure, crate::OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let plan_generator = generator();
    recompute_post_task_closure(
        &PostTaskClosureContext {
            root,
            discovery,
            catalog: &catalog,
            generator: &plan_generator,
            run_key: RUN_KEY,
            logical_runner_label,
        },
        matrix_key,
        entry,
    )
}

#[test]
fn current_rust_source_matches_the_planned_identity() {
    let root = TempDir::new().expect("repository");
    write_rust_source(root.path());
    let task = rust_task();
    let discovery = discovery_with_proposals(vec![task.clone()]);
    let (entry, obligation) =
        planned_adapter(root.path(), &discovery, &task, "rust-demo", RUST_RUNNER);

    let observed = observed_rust(root.path(), &discovery, &entry);

    require_plan_match(&observed, &entry, &obligation).expect("same source identity");
    assert_eq!(observed.observed_input_digest, entry.input_digest);
    assert_eq!(observed.observed_closure_digest, obligation.closure_digest);
}

#[test]
fn source_edit_changes_observed_closure_without_copying_the_plan_digest() {
    let root = TempDir::new().expect("repository");
    write_rust_source(root.path());
    let task = rust_task();
    let discovery = discovery_with_proposals(vec![task.clone()]);
    let (entry, obligation) =
        planned_adapter(root.path(), &discovery, &task, "rust-demo", RUST_RUNNER);
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .expect("edit source");

    let observed = observed_rust(root.path(), &discovery, &entry);

    assert_ne!(observed.observed_closure_digest, obligation.closure_digest);
    assert_ne!(observed.observed_input_digest, entry.input_digest);
    assert!(require_plan_match(&observed, &entry, &obligation).is_err());
}

#[test]
fn changed_task_inputs_change_the_observed_identity() {
    let root = TempDir::new().expect("repository");
    write_rust_source(root.path());
    let task = rust_task();
    let discovery = discovery_with_proposals(vec![task.clone()]);
    let (entry, obligation) =
        planned_adapter(root.path(), &discovery, &task, "rust-demo", RUST_RUNNER);
    let mut changed_task = task;
    changed_task.identity.features = vec!["serde".to_owned()];
    let changed_discovery = discovery_with_proposals(vec![changed_task]);

    let observed = observed_rust(root.path(), &changed_discovery, &entry);

    assert_ne!(observed.observed_input_digest, entry.input_digest);
    assert!(require_plan_match(&observed, &entry, &obligation).is_err());
}

#[test]
fn a_different_matrix_key_cannot_select_the_observation() {
    let root = TempDir::new().expect("repository");
    write_rust_source(root.path());
    let task = rust_task();
    let discovery = discovery_with_proposals(vec![task.clone()]);
    let (entry, _) = planned_adapter(root.path(), &discovery, &task, "rust-demo", RUST_RUNNER);

    let error = observed_for(
        root.path(),
        &discovery,
        RUST_RUNNER,
        "m-0000000000000000",
        &entry,
    )
    .expect_err("wrong matrix lane");

    assert!(
        error
            .to_string()
            .contains("qualification_closure_matrix_key_mismatch")
    );
}

#[test]
fn tofu_scale_set_lane_uses_the_logical_plan_runner_for_identity() {
    let root = TempDir::new().expect("repository");
    std::fs::write(
        root.path().join(".terraform.lock.hcl"),
        "provider lock one\n",
    )
    .expect("lockfile");
    let task = tofu_task();
    let discovery = discovery_with_proposals(vec![task.clone()]);
    let (entry, obligation) = planned_adapter(
        root.path(),
        &discovery,
        &task,
        "tofu-demo",
        "scale-set:velnor+orbstack-linux",
    );

    let observed = observed_for(
        root.path(),
        &discovery,
        RUST_RUNNER,
        &entry.matrix_key,
        &entry,
    )
    .expect("ToFu closure");

    require_plan_match(&observed, &entry, &obligation).expect("same ToFu source");
}

#[test]
fn named_check_reuses_its_task_and_input_closure_derivation() {
    let root = TempDir::new().expect("repository");
    std::fs::write(
        root.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo first'\n",
    )
    .expect("Mise task");
    std::fs::write(root.path().join("input.txt"), "input one\n").expect("check input");
    let check = MiseCheck {
        id: "verify".to_owned(),
        task: "verify".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["input.txt".to_owned()],
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 5,
    };
    let (planned_discovery, planned_check) = check_discovery(root.path(), &check);
    let (obligation, entry) = crate::internal_plan::named_checks::plan::derive(
        root.path(),
        &planned_check,
        RUN_KEY,
        &generator(),
        &ToolCatalog::pinned(),
    )
    .expect("planned named check");

    let observed = observed_for(
        root.path(),
        &planned_discovery,
        RUST_RUNNER,
        &entry.matrix_key,
        &entry,
    )
    .expect("named-check closure");

    require_plan_match(&observed, &entry, &obligation).expect("same named-check input");
    std::fs::write(
        root.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo second'\n",
    )
    .expect("edit task config");
    let (current_discovery, _) = check_discovery(root.path(), &check);

    let observed = observed_for(
        root.path(),
        &current_discovery,
        RUST_RUNNER,
        &entry.matrix_key,
        &entry,
    )
    .expect("recomputed named-check closure");

    assert_ne!(observed.observed_input_digest, entry.input_digest);
    assert_ne!(observed.observed_closure_digest, obligation.closure_digest);
    assert!(require_plan_match(&observed, &entry, &obligation).is_err());
}
