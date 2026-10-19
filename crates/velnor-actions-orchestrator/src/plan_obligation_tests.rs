//! Obligation extension-dispatch tests.
//!
//! Declared via `#[path]` from `plan_obligation.rs` under `cfg(test)`.

use std::path::PathBuf;

use super::*;
use velnor_actions_contract::cachekey::{RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA};
use velnor_actions_contract::{NamedCheckLaneVariant, WorkflowEvent, digest_b3};

const HOSTED_RUNNER: &str = "ubuntu-26.04";
const SCALE_SET_RUNNER: &str = "scale-set:velnor+orbstack-linux";

/// Minimal discovery with no workspaces or tool checks.
fn empty_discovery() -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
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
    }
}

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

fn rust_proposal() -> ProposedTask {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
    let group = TaskGroup {
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
    };
    let task = velnor_actions_rust::propose_task(&group).expect("Rust proposal");
    task.validate().expect("valid Rust proposal");
    task
}

fn qualification_lanes(task: &ProposedTask) -> Vec<NamedCheckLane> {
    let id = crate::crate_job_ids::job_id_for_member(
        std::slice::from_ref(task),
        task,
        WorkflowEvent::Qualification,
    )
    .expect("valid owner lookup")
    .expect("shared qualification job identity");
    vec![
        NamedCheckLane {
            variant: Some(NamedCheckLaneVariant::Hosted),
            job_id: format!("{id}__hosted"),
            runs_on: Some(HOSTED_RUNNER.to_owned()),
        },
        NamedCheckLane {
            variant: Some(NamedCheckLaneVariant::ScaleSet),
            job_id: format!("{id}__local"),
            runs_on: Some(SCALE_SET_RUNNER.to_owned()),
        },
    ]
}

fn qualification_matrix_entries(task: &ProposedTask) -> Vec<MatrixEntry> {
    let catalog = ToolCatalog::pinned();
    let toolchain_inputs = crate::internal_plan::identities::toolchain_inputs_for(task, &catalog)
        .expect("toolchain inputs");
    let toolchain =
        velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs).expect("toolchain ID");
    let argv = vec!["true".to_owned()];
    let digest = task_digest(&task.task_id, &argv, &toolchain).expect("task digest");
    let metadata = serde_json::json!({});
    let execute_task_ids = execute_ids(task);
    let input_digest = digest_b3(b"qualification-input");
    derive_lane_entries(
        &LaneEntryInputs {
            task,
            run: "true",
            task_digest: &digest,
            metadata: &metadata,
            execute_task_ids: &execute_task_ids,
            input_digest: &input_digest,
            run_key: "local",
            target: "x86_64-unknown-linux-gnu",
            default_runs_on: HOSTED_RUNNER,
            toolchain: &toolchain,
            toolchain_inputs: &toolchain_inputs,
        },
        qualification_lanes(task),
    )
    .expect("derive hosted and Scale Set entries")
}

/// Unique temporary directory removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create(label: &str) -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "velnor-plan-obligation-{label}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

#[test]
fn tofu_tasks_derive_the_tofu_envelope() {
    use velnor_actions_tofu::TofuTaskKind;
    let dir = TempDir::create("tofu-ext").expect("tempdir");
    std::fs::write(dir.path.join(".terraform.lock.hcl"), "lock").expect("lockfile");
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let (envelope, eligible) = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect("derives");
    assert_eq!(envelope.schema, TOFU_EXTENSION_SCHEMA);
    assert!(
        !eligible,
        "T23: validate reuse is OFF despite a known lockfile"
    );
    assert!(velnor_actions_contract::validate_tofu_extension(&envelope).is_ok());
}

#[test]
fn tofu_drift_fails_the_bridge_closed() {
    use velnor_actions_tofu::TofuTaskKind;
    let dir = TempDir::create("tofu-drift").expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::InitForValidate);
    task.identity.compile_driver = "cargo".to_owned();
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let err = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect_err("drift fails");
    assert!(err.to_string().contains("unknown_driver:cargo"), "{err}");
}

#[test]
fn rust_tasks_keep_the_rust_envelope() {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
    let group = TaskGroup {
        all_features: false,
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo".to_owned(),
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
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    let dir = TempDir::create("rust-ext").expect("tempdir");
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let (envelope, eligible) = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect("derives");
    assert_eq!(envelope.schema, RUST_EXTENSION_SCHEMA);
    assert!(eligible, "no build script is reuse-eligible");
}

#[test]
fn both_qualification_lanes_persist_distinct_rust_and_tofu_matrix_entries() {
    use velnor_actions_tofu::TofuTaskKind;
    for task in [rust_proposal(), tofu_proposal(TofuTaskKind::Validate)] {
        let entries = qualification_matrix_entries(&task);
        assert_eq!(entries.len(), 2);
        let hosted = &entries[0];
        let scale_set = &entries[1];
        assert_eq!(hosted.lane_variant, Some(NamedCheckLaneVariant::Hosted));
        assert_eq!(
            scale_set.lane_variant,
            Some(NamedCheckLaneVariant::ScaleSet)
        );
        assert!(hosted.job_id.ends_with("__hosted"));
        assert!(scale_set.job_id.ends_with("__local"));
        assert_eq!(hosted.task_id, scale_set.task_id);
        assert_ne!(hosted.matrix_key, scale_set.matrix_key);
        assert_ne!(hosted.report_id, scale_set.report_id);
        assert_ne!(hosted.artifact_id, scale_set.artifact_id);
        for entry in entries {
            entry.validate("local").expect("valid lane entry");
            assert_eq!(
                entry.cache_ids.as_ref().expect("cache IDs").platform_id(),
                entry.planned_platform.platform_id
            );
        }
    }
}
