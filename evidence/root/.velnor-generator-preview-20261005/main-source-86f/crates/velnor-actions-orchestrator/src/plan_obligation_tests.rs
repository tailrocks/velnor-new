//! Obligation extension-dispatch tests.
//!
//! Declared via `#[path]` from `plan_obligation.rs` under `cfg(test)`.

use std::path::PathBuf;

use super::*;
use velnor_actions_contract::cachekey::{RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA};

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
