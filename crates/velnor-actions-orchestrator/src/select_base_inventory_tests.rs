//! Authenticated inventory graph reuse without Git projection or Cargo.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;
use crate::discover::PlannedWorkspace;
use velnor_actions_rust::{
    CompileDriver, PackageRecord, ProfileSource, RustExecutionProfile, SkippedPathEdge, TestRunner,
};

fn package(dir: &str, root: &Path) -> PackageRecord {
    let root_uri = root
        .to_string_lossy()
        .replace('%', "%25")
        .replace(' ', "%20");
    PackageRecord {
        id: format!("path+file://{root_uri}/{dir}#0.1.0"),
        name: dir.to_owned(),
        version: "0.1.0".to_owned(),
        manifest: format!("{dir}/Cargo.toml"),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: Vec::new(),
        has_build_script: false,
    }
}

fn record(root: &Path) -> WorkspaceRecord {
    let packages = vec![package("alpha", root), package("beta", root)];
    WorkspaceRecord {
        workspace_root: String::new(),
        members: packages.iter().map(|package| package.id.clone()).collect(),
        skipped_edges: vec![SkippedPathEdge {
            from: packages[0].id.clone(),
            path: root.join("beta").to_string_lossy().into_owned(),
            kind: velnor_actions_rust::DepKind::Build,
            optional: false,
            target: Some("cfg(unix)".to_owned()),
        }],
        packages,
        edges: Vec::new(),
    }
}

fn discovery(record: WorkspaceRecord) -> Discovery {
    Discovery {
        rust_inventory: None,
        raw_inventories: Vec::new(),
        statuses: Vec::new(),
        workspaces: vec![PlannedWorkspace {
            record,
            profile: RustExecutionProfile {
                compile_driver: CompileDriver::Cargo,
                test_runner: TestRunner::CargoTest,
                evidence: Vec::new(),
                driver_source: ProfileSource::Detected,
                runner_source: ProfileSource::Detected,
                nextest_profile: velnor_actions_rust::NextestProfile::Default,
                nextest_config: None,
            },
            recommendations: Vec::new(),
            findings: Vec::new(),
        }],
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
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

fn fixture() -> Result<tempfile::TempDir, String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    fixture_at(root.path())?;
    Ok(root)
}

fn fixture_at(root: &Path) -> Result<(), String> {
    for manifest in ["Cargo.toml", "alpha/Cargo.toml", "beta/Cargo.toml"] {
        let path = root.join(manifest);
        let parent = path.parent().ok_or_else(|| "missing parent".to_owned())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        std::fs::write(path, "[workspace]\n").map_err(|error| error.to_string())?;
    }
    for package in ["alpha", "beta"] {
        std::fs::create_dir_all(root.join(package).join("src"))
            .map_err(|error| error.to_string())?;
        std::fs::write(root.join(package).join("src/lib.rs"), "old source")
            .map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(root.join(".velnor")).map_err(|error| error.to_string())?;
    std::fs::write(root.join(".velnor/config.toml"), "schema = 1\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(root.join("README.md"), "old documentation")
        .map_err(|error| error.to_string())?;
    std::fs::write(root.join("alpha/input.rs"), "old source").map_err(|error| error.to_string())?;
    Ok(())
}

fn authenticated(root: &Path, base: &str) -> Result<Discovery, String> {
    authenticated_from(root, root, base)
}

fn authenticated_from(publisher: &Path, root: &Path, base: &str) -> Result<Discovery, String> {
    use crate::analysis_inventory::{
        AnalysisIdentity, AnalysisSource, build_payload, parse_authenticated,
        resolution_inputs_digest,
    };
    let publisher = publisher
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let inventories = vec![("Cargo.toml".to_owned(), record(&publisher))];
    let (index, _) =
        crate::discover_index::build_file_index(&root, &[]).map_err(|error| error.to_string())?;
    let cargo_pin = velnor_actions_mise::ToolCatalog::pinned()
        .rustup_toolchain()
        .to_owned();
    let identity = AnalysisIdentity {
        helper_sha256: "c".repeat(64),
        cargo_identity: "cargo 1.98.1 (797e8a9bc 2026-08-05)".to_owned(),
        cargo_pin,
        resolution_inputs_digest: resolution_inputs_digest(&root, index.files(), &inventories)?,
        source: AnalysisSource {
            repository: "fixture/repository".to_owned(),
            head_sha: base.to_owned(),
            workflow_sha: base.to_owned(),
            run_id: 1,
            run_attempt: 1,
            branch: "main".to_owned(),
        },
    };
    let text = build_payload(&publisher, identity.clone(), &inventories)?;
    let authority =
        crate::analysis_inventory::authority::RemoteAnalysisAuthority::fixture(identity, &text);
    let mut discovery = discovery(record(&root));
    discovery.rust_inventory = Some(parse_authenticated(
        &root,
        index.files(),
        &text,
        &authority,
    )?);
    Ok(discovery)
}

#[test]
fn relocated_checkout_uri_escaping_matches_candidate_package_ids() -> Result<(), String> {
    let publisher = fixture()?;
    let candidate = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = candidate.path().join("checkout with % space");
    fixture_at(&root)?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let base = "a".repeat(40);
    let discovery = authenticated_from(publisher.path(), &root, &base)?;
    let graph = base_graph(&root, &base, &discovery)?;
    assert_eq!(graph.owners, candidate_graph(&root, &discovery)?.owners);
    assert!(
        graph
            .owners
            .iter()
            .all(|(_, id)| id.contains("checkout%20with%20%25%20space"))
    );
    assert_eq!(graph.edges.len(), 1);
    Ok(())
}

#[test]
fn relocated_authenticated_graph_uses_current_ids_and_cross_workspace_paths() -> Result<(), String>
{
    let publisher = fixture()?;
    let candidate = fixture()?;
    let root = candidate
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let base = "a".repeat(40);
    let discovery = authenticated_from(publisher.path(), &root, &base)?;
    let graph = base_graph(&root, &base, &discovery)?;
    assert_eq!(graph.owners, candidate_graph(&root, &discovery)?.owners);
    assert!(
        graph
            .owners
            .iter()
            .all(|(_, id)| id.contains(&root.display().to_string()))
    );
    assert_eq!(graph.edges.len(), 1);
    Ok(())
}

#[test]
fn exact_authenticated_base_survives_docs_and_source_edits_without_cargo() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let base = "a".repeat(40);
    let discovery = authenticated(&root, &base)?;
    std::fs::write(root.join("README.md"), "new documentation")
        .map_err(|error| error.to_string())?;
    std::fs::write(root.join("alpha/input.rs"), "new source").map_err(|error| error.to_string())?;
    // No Git repository exists: materialization would fail before Cargo.
    let graph = base_graph(&root, &base, &discovery)?;
    let owners: BTreeMap<_, _> = graph.owners.into_iter().collect();
    assert_eq!(
        graph.edges,
        vec![(owners["alpha"].clone(), owners["beta"].clone())]
    );
    assert!(owners.values().all(|id| !id.starts_with("base:")));
    Ok(())
}

#[test]
fn stale_base_and_other_checkout_cannot_use_authenticated_graph() -> Result<(), String> {
    let repository = fixture()?;
    let root = repository
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let base = "a".repeat(40);
    let discovery = authenticated(&root, &base)?;
    assert!(base_graph(&root, &"b".repeat(40), &discovery).is_err());
    let elsewhere = fixture()?;
    assert!(base_graph(elsewhere.path(), &base, &discovery).is_err());
    Ok(())
}

#[test]
fn changed_resolution_inputs_reject_cached_graph() -> Result<(), String> {
    for change in [
        "dependency",
        "target",
        "target_delete",
        "target_rename",
        "package_delete",
        "package_rename",
    ] {
        let fixture = fixture()?;
        let root = fixture
            .path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let base = "a".repeat(40);
        let discovery = authenticated(&root, &base)?;
        if change == "dependency" {
            std::fs::write(
                root.join("alpha/Cargo.toml"),
                "[dependencies]\nnew = '=1.0.0'\n",
            )
            .map_err(|error| error.to_string())?;
        } else if change == "target" {
            std::fs::create_dir_all(root.join("alpha/src/bin"))
                .map_err(|error| error.to_string())?;
            std::fs::write(root.join("alpha/src/bin/new.rs"), "fn main() {}")
                .map_err(|error| error.to_string())?;
        } else {
            let (from, to) = match change {
                "target_delete" => ("alpha/src/lib.rs", None),
                "target_rename" => ("alpha/src/lib.rs", Some("alpha/src/main.rs")),
                "package_delete" => ("beta/Cargo.toml", None),
                "package_rename" => ("beta/Cargo.toml", Some("beta/Renamed.toml")),
                _ => return Err("unknown fixture change".to_owned()),
            };
            if let Some(to) = to {
                std::fs::rename(root.join(from), root.join(to))
                    .map_err(|error| error.to_string())?;
            } else {
                std::fs::remove_file(root.join(from)).map_err(|error| error.to_string())?;
            }
        }
        assert_eq!(
            base_graph(&root, &base, &discovery).err().as_deref(),
            Some("analysis_inventory_inputs_changed")
        );
    }
    Ok(())
}

#[test]
fn documentation_and_source_bytes_do_not_require_candidate_metadata() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture.path();
    let discovery = discovery(record(root));
    let before = candidate_graph(root, &discovery)?;
    std::fs::write(root.join("README.md"), "new documentation")
        .map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("alpha/input.rs"),
        "compile_error!(\"source changed\");",
    )
    .map_err(|error| error.to_string())?;
    let after = candidate_graph(root, &discovery)?;
    assert_eq!(before.owners, after.owners);
    assert_eq!(before.edges, after.edges);
    Ok(())
}

#[test]
fn rebound_records_retain_cross_workspace_edges_and_removed_owners() -> Result<(), String> {
    let root = fixture()?;
    let inventory = record(root.path());
    let graph = graph_from_records(root.path(), &[&inventory])?;
    let candidate = PackageGraph {
        owners: vec![("alpha".to_owned(), "candidate-alpha".to_owned())],
        edges: Vec::new(),
    };
    let remapped = remap_graph(root.path(), root.path(), graph, candidate)?;
    let owners: BTreeMap<_, _> = remapped.owners.iter().cloned().collect();
    assert_eq!(owners["alpha"], "candidate-alpha");
    assert!(owners["beta"].starts_with("base:"));
    assert_eq!(
        remapped.edges,
        vec![("candidate-alpha".to_owned(), owners["beta"].clone())]
    );
    Ok(())
}
