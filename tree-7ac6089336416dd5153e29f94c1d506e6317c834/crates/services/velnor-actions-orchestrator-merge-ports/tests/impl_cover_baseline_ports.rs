use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract_planning::{
    CachePolicy, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_merge_ports::{
    BaselineManifest, CoverBaselinePort, changed_keys, member_changed,
};

fn manifest(run_id: u64) -> BaselineManifest {
    serde_json::from_value(serde_json::json!({
        "schema": 2,
        "repository_id": "repo",
        "source_commit": "abc",
        "ref": "refs/heads/main",
        "event": "push",
        "workflow_ref": "w",
        "run_id": run_id,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": "v",
        "generator_sha256": "s",
        "compatibility_id": "c",
        "artifact_id": 9,
        "artifact_name": "n",
        "tasks": [],
    }))
    .expect("manifest")
}

struct StubBaseline {
    manifests: Result<Vec<BaselineManifest>, String>,
}

impl CoverBaselinePort for StubBaseline {
    fn resolve_manifests(
        &self,
        _catalog: &ToolCatalog,
        _root: &Path,
        _base: &str,
        _workflow: &str,
        _branch: &str,
        _artifact: Option<&str>,
        _repository: Option<&str>,
    ) -> Result<Vec<BaselineManifest>, String> {
        self.manifests.clone()
    }
}

fn resolve(port: &dyn CoverBaselinePort) -> Result<Vec<BaselineManifest>, String> {
    let catalog = ToolCatalog::pinned();
    port.resolve_manifests(
        &catalog,
        Path::new("checkout"),
        &"a".repeat(40),
        ".github/workflows/ci.yml",
        "main",
        Some("artifact"),
        Some("owner/repo"),
    )
}

#[test]
fn port_resolve_ok_returns_canned_manifests() {
    let port: &dyn CoverBaselinePort = &StubBaseline {
        manifests: Ok(vec![manifest(7), manifest(8)]),
    };
    let found = resolve(port).expect("manifests");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].run_id, 7);
    assert_eq!(found[1].run_id, 8);
}

#[test]
fn port_resolve_err_passes_reason_verbatim() {
    let port: &dyn CoverBaselinePort = &StubBaseline {
        manifests: Err("baseline_not_found".to_owned()),
    };
    assert_eq!(resolve(port).expect_err("must fail"), "baseline_not_found");
}

#[test]
fn port_resolve_empty_ok_means_no_manifest() {
    let port: &dyn CoverBaselinePort = &StubBaseline {
        manifests: Ok(vec![]),
    };
    assert!(resolve(port).expect("empty").is_empty());
}

#[test]
fn port_dispatches_through_object_safe_reference() {
    let stub = StubBaseline {
        manifests: Err("baseline_no_exact_artifact".to_owned()),
    };
    let port: &dyn CoverBaselinePort = &stub;
    assert_eq!(
        resolve(port).expect_err("must fail"),
        "baseline_no_exact_artifact"
    );
}

/// Minimal proposal with controlled unit identity.
fn proposal(unit_id: &str, unit_key: &str) -> ProposedTask {
    ProposedTask {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        stack_id: "rust".to_owned(),
        component_id: "demo@0.1.0".to_owned(),
        task_kind: "clippy".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: vec!["Cargo.toml".to_owned()],
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
            allow_compilation_reuse: true,
            allow_task_reuse: true,
        },
        identity: IdentityInputs {
            unit_id: unit_id.to_owned(),
            unit_key: unit_key.to_owned(),
            unit_path: "Cargo.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
            environment: std::collections::BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("clippy")],
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn changed_keys_empty_universe_is_empty() {
    assert!(changed_keys(&[], &set(&["demo"])).is_empty());
}

#[test]
fn changed_keys_collects_unit_keys_of_changed_units() {
    let changed = proposal("demo", "root");
    let same = proposal("demo", "other");
    let untouched = proposal("other", "elsewhere");
    let universe = vec![&changed, &same, &untouched];
    assert_eq!(
        changed_keys(&universe, &set(&["demo"])),
        set(&["root", "other"])
    );
}

#[test]
fn changed_keys_ignores_units_outside_changed_set() {
    let task = proposal("demo", "root");
    assert!(changed_keys(&[&task], &set(&["unrelated"])).is_empty());
}

#[test]
fn member_changed_without_hint_counts_as_changed() {
    let task = proposal("demo", "root");
    assert!(member_changed(&task, None, &set(&[])));
}

#[test]
fn member_changed_unit_hit_counts_as_changed() {
    let task = proposal("demo", "root");
    assert!(member_changed(&task, Some(&set(&["demo"])), &set(&[])));
    assert!(!member_changed(&task, Some(&set(&["other"])), &set(&[])));
}

#[test]
fn member_changed_empty_unit_falls_back_to_keys() {
    let task = proposal("", "root");
    assert!(member_changed(
        &task,
        Some(&set(&["unrelated"])),
        &set(&["root"])
    ));
    assert!(!member_changed(
        &task,
        Some(&set(&["unrelated"])),
        &set(&["elsewhere"])
    ));
}
