//! Authenticated preparation retains normal discovery without invoking Cargo.

use std::path::Path;

use velnor_actions_rust::{PackageRecord, TargetRecord, WorkspaceRecord};

use super::*;
use crate::analysis_inventory::authority::RemoteAnalysisAuthority;
use crate::analysis_inventory::{
    AnalysisIdentity, AnalysisSource, ValidatedInventory, build_payload, parse_authenticated,
    resolution_inputs_digest,
};
use crate::inventory::cargo_probe::CargoProbe;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    repo: tempfile::TempDir,
    identity: AnalysisIdentity,
    text: String,
    inventory: ValidatedInventory,
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    let root = repo.path().canonicalize()?;
    std::fs::create_dir_all(root.join(".velnor"))?;
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n",
    )?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    std::fs::write(
        root.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    std::fs::write(root.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n")?;
    std::fs::write(root.join("README.md"), "Unrelated prose.\n")?;
    let id = format!("path+file://{}#demo@0.1.0", root.display());
    let record = WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![id.clone()],
        packages: vec![PackageRecord {
            id,
            name: "demo".to_owned(),
            version: "0.1.0".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: vec![TargetRecord {
                kind: "lib".to_owned(),
                name: "demo".to_owned(),
                test: true,
                doctest: true,
                required_features: Vec::new(),
            }],
            features: Vec::new(),
            has_build_script: false,
        }],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
    };
    let inventories = vec![("Cargo.toml".to_owned(), record)];
    let files = files(&root)?;
    let pin = velnor_actions_mise::ToolCatalog::pinned()
        .rustup_toolchain()
        .to_owned();
    let identity = AnalysisIdentity {
        helper_sha256: "a".repeat(64),
        cargo_identity: crate::analysis_inventory::QUALIFIED_CARGO_TEST_IDENTITY.to_owned(),
        cargo_pin: pin,
        resolution_inputs_digest: resolution_inputs_digest(&root, &files, &inventories)?,
        source: AnalysisSource {
            repository: "example/demo".to_owned(),
            head_sha: "b".repeat(40),
            workflow_sha: "c".repeat(40),
            run_id: 1,
            run_attempt: 1,
            branch: "main".to_owned(),
        },
    };
    let text = build_payload(&root, identity.clone(), &inventories)?;
    let authority = RemoteAnalysisAuthority::fixture(identity.clone(), &text);
    let inventory = parse_authenticated(&root, &files, &text, &authority)?;
    Ok(Fixture {
        repo,
        identity,
        text,
        inventory,
    })
}

fn files(root: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(velnor_actions_contract::build_index(root, &[])?
        .files()
        .to_vec())
}

fn cached(fixture: &Fixture) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_with_inventory(
        fixture.repo.path(),
        InventoryProvider::ValidatedInventory(&fixture.inventory),
    )
}

#[test]
fn docs_only_preparation_preserves_records_profiles_and_proposals_without_cargo() -> TestResult {
    let fixture = fixture()?;
    let probe = CargoProbe::begin();
    let before = cached(&fixture)?;
    std::fs::write(
        fixture.repo.path().join("README.md"),
        "Changed unrelated prose.\n",
    )?;
    let after = cached(&fixture)?;
    assert_eq!(
        probe.attempts(),
        0,
        "discovery and qualification must skip Cargo"
    );
    assert_eq!(before.discovery.workspaces.len(), 1);
    assert_eq!(
        before.discovery.workspaces[0].record,
        after.discovery.workspaces[0].record
    );
    assert_eq!(
        before.discovery.workspaces[0].profile,
        after.discovery.workspaces[0].profile
    );
    assert_eq!(before.discovery.proposals, after.discovery.proposals);
    let kinds: Vec<_> = after
        .discovery
        .proposals
        .iter()
        .filter(|task| task.stack_id == "rust")
        .map(|task| task.task_kind.as_str())
        .collect();
    assert_eq!(kinds, ["clippy", "doc", "doctest", "test"]);
    assert_eq!(after.discovery.raw_inventories.len(), 1);
    assert!(after.discovery.rust_inventory.is_some());
    Ok(())
}

#[test]
fn source_edits_reuse_resolution_but_keep_current_source_discovery() -> TestResult {
    let fixture = fixture()?;
    let probe = CargoProbe::begin();
    std::fs::write(
        fixture.repo.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 43 }\n",
    )?;
    let prepared = cached(&fixture)?;
    assert_eq!(probe.attempts(), 0);
    assert_eq!(prepared.discovery.proposals.len(), 4);
    Ok(())
}

#[test]
fn changed_resolution_inputs_require_cargo_before_any_invocation() -> TestResult {
    for (path, body) in [
        (
            "Cargo.toml",
            "[package]\nname = \"renamed\"\nversion = \"0.1.0\"\n",
        ),
        ("Cargo.lock", "version = 4\n"),
        ("src/main.rs", "fn main() {}\n"),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"nightly\"\n",
        ),
    ] {
        let fixture = fixture()?;
        let probe = CargoProbe::begin();
        std::fs::write(fixture.repo.path().join(path), body)?;
        assert!(
            matches!(cached(&fixture), Err(OrchestratorError::NeedsCargo { .. })),
            "{path}"
        );
        assert_eq!(probe.attempts(), 0, "{path}: fallback belongs to caller");
    }
    Ok(())
}

#[test]
fn wrong_checkout_requires_cargo() -> TestResult {
    let first = fixture()?;
    let second = fixture()?;
    let probe = CargoProbe::begin();
    let result = prepare_with_inventory(
        second.repo.path(),
        InventoryProvider::ValidatedInventory(&first.inventory),
    );
    assert!(matches!(result, Err(OrchestratorError::NeedsCargo { .. })));
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn missing_candidate_is_typed_cargo_fallback() -> TestResult {
    let fixture = fixture()?;
    let result = InventoryProvider::ValidatedInventory(&fixture.inventory)
        .cached_inventories(&["other/Cargo.toml".to_owned()]);
    assert!(
        matches!(result, Err(OrchestratorError::NeedsCargo { problem })
        if problem == "inventory_missing_manifest:other/Cargo.toml")
    );
    Ok(())
}

#[test]
fn source_and_catalog_identity_mismatch_never_construct_proof() -> TestResult {
    for field in ["head_sha", "cargo_pin", "helper_sha256"] {
        let fixture = fixture()?;
        let mut value: serde_json::Value = serde_json::from_str(&fixture.text)?;
        if field == "head_sha" {
            value["identity"]["source"][field] = serde_json::Value::String("d".repeat(40));
        } else {
            let replacement = if field == "cargo_pin" {
                "0.0.0".to_owned()
            } else {
                "d".repeat(64)
            };
            value["identity"][field] = serde_json::Value::String(replacement);
        }
        let text = serde_json::to_string(&value)?;
        let authority = RemoteAnalysisAuthority::fixture(fixture.identity.clone(), &text);
        let result = parse_authenticated(
            fixture.repo.path(),
            &files(fixture.repo.path())?,
            &text,
            &authority,
        )
        .map_err(|problem| OrchestratorError::NeedsCargo { problem });
        assert!(
            matches!(result, Err(OrchestratorError::NeedsCargo { .. })),
            "{field}"
        );
    }
    Ok(())
}

#[test]
fn base_inventory_is_available_only_for_authenticated_exact_base() -> TestResult {
    let fixture = fixture()?;
    assert!(
        fixture
            .inventory
            .base_records(&fixture.identity.source.head_sha)
            .is_some()
    );
    assert!(fixture.inventory.base_records(&"d".repeat(40)).is_none());
    Ok(())
}

#[test]
fn cargo_forbidden_admission_rejects_new_rust_before_invocation() -> TestResult {
    let fixture = fixture()?;
    let probe = CargoProbe::begin();
    let result = prepare_with_inventory(fixture.repo.path(), InventoryProvider::FreshWithoutCargo);
    assert!(
        matches!(result, Err(OrchestratorError::NeedsCargo { problem })
        if problem == "rust_discovered_after_no_cargo_admission")
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn cargo_forbidden_admission_runs_fresh_non_rust_discovery() -> TestResult {
    let fixture = fixture()?;
    std::fs::remove_file(fixture.repo.path().join("Cargo.toml"))?;
    std::fs::remove_file(fixture.repo.path().join("Cargo.lock"))?;
    let probe = CargoProbe::begin();
    let prepared =
        prepare_with_inventory(fixture.repo.path(), InventoryProvider::FreshWithoutCargo)?;
    assert_eq!(probe.attempts(), 0);
    assert!(prepared.discovery.workspaces.is_empty());
    assert!(prepared.discovery.statuses.is_empty());
    assert!(prepared.discovery.rust_inventory.is_none());
    Ok(())
}
