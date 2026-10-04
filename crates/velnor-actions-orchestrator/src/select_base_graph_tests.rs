//! Base inventory semantics, removed owners, and incomplete resolution guards.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::collections::BTreeSet;
use std::fs;

use velnor_actions_contract::reverse_closure;
use velnor_actions_rust::DepKind;

use super::*;

fn fixture() -> Result<tempfile::TempDir, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::write(dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = ['alpha', 'beta', 'gamma']\nresolver = '3'\n[workspace.dependencies]\nbeta = { path = 'beta' }\n")
        .map_err(|error| error.to_string())?;
    for name in ["alpha", "beta", "gamma"] {
        let root = dir.path().join(name);
        fs::create_dir_all(root.join("src")).map_err(|error| error.to_string())?;
        fs::write(root.join("src/lib.rs"), "").map_err(|error| error.to_string())?;
        let dependencies = if name == "alpha" {
            "[dependencies]\nbeta = { workspace = true, optional = true }\n[build-dependencies]\nbeta.workspace = true\n[dev-dependencies]\nbeta.workspace = true\n[target.'cfg(windows)'.dependencies]\nbeta.workspace = true\n"
        } else {
            ""
        };
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2024'\n{dependencies}"
            ),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(dir)
}

fn manifest_set() -> BTreeSet<String> {
    [
        "Cargo.toml",
        "alpha/Cargo.toml",
        "beta/Cargo.toml",
        "gamma/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|error| error.to_string())
}

#[test]
fn committed_base_inventory_retains_deleted_package_and_renamed_consumer() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join(".velnor")).map_err(|error| error.to_string())?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\ndefault_branch = 'testmain'\n",
    )
    .map_err(|error| error.to_string())?;
    git(&root, &["init", "-b", "testmain"])?;
    git(&root, &["config", "user.email", "test@example.com"])?;
    git(&root, &["config", "user.name", "Test"])?;
    git(&root, &["config", "commit.gpgsign", "false"])?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-m", "base"])?;
    let base = git(&root, &["rev-parse", "HEAD"])?;
    fs::remove_dir_all(root.join("beta")).map_err(|error| error.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = ['alpha', 'gamma']\nresolver = '3'\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = 'renamed-consumer'\nversion = '2.0.0'\nedition = '2024'\n",
    )
    .map_err(|error| error.to_string())?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-m", "candidate"])?;
    let discovery = crate::prepare(&root)
        .map_err(|error| error.to_string())?
        .discovery;
    let candidate = candidate_graph(&root, &discovery)?;
    let current: BTreeMap<_, _> = candidate.owners.into_iter().collect();
    let old = base_graph(&root, &base, &discovery)?;
    let old_owners: BTreeMap<_, _> = old.owners.iter().cloned().collect();
    assert_eq!(
        old_owners["alpha"], current["alpha"],
        "manifest binds actual changed Cargo ID"
    );
    assert!(old_owners["beta"].starts_with("base:"));
    let removed = BTreeSet::from([old_owners["beta"].clone()]);
    let affected = reverse_closure(&old.edges, &candidate.edges, &removed);
    assert!(
        affected.contains(&current["alpha"]),
        "old inherited edge reaches surviving consumer"
    );
    assert!(
        !affected.contains(&current["gamma"]),
        "unrelated leaf remains unchanged"
    );
    Ok(())
}

#[test]
fn immutable_inventory_resolves_inheritance_and_all_declared_edge_kinds() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let records = inventories(&root, &manifest_set())?;
    assert_eq!(records.len(), 1, "workspace members reuse exact inventory");
    let edges = &records[0].edges;
    let kinds: BTreeSet<_> = edges.iter().map(|edge| edge.kind).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([DepKind::Normal, DepKind::Build, DepKind::Dev])
    );
    assert!(edges.iter().any(|edge| edge.optional));
    assert!(
        edges
            .iter()
            .any(|edge| edge.target.as_deref() == Some("cfg(windows)"))
    );
    let graph = graph_from_records(&root, &[&records[0]])?;
    let ids: BTreeMap<_, _> = graph.owners.iter().cloned().collect();
    assert_eq!(graph.edges, [(ids["alpha"].clone(), ids["beta"].clone())]);
    let leaf = BTreeSet::from([ids["gamma"].clone()]);
    assert_eq!(
        reverse_closure(&graph.edges, &[], &leaf),
        leaf,
        "true leaf stays narrow"
    );
    Ok(())
}

#[test]
fn removed_package_owners_and_edges_survive_candidate_id_translation() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let records = inventories(&root, &manifest_set())?;
    let base = graph_from_records(&root, &[&records[0]])?;
    let candidate = PackageGraph {
        owners: vec![
            ("alpha".to_owned(), "candidate-renamed-alpha@2".to_owned()),
            ("gamma".to_owned(), "candidate-gamma".to_owned()),
        ],
        edges: Vec::new(),
    };
    let base = remap_graph(Path::new("/repository"), &root, base, candidate)?;
    let owners: BTreeMap<_, _> = base.owners.iter().cloned().collect();
    assert_eq!(
        owners["alpha"], "candidate-renamed-alpha@2",
        "manifest binds name/version changes"
    );
    assert!(
        owners["beta"].starts_with("base:"),
        "removed owner retained"
    );
    let removed = BTreeSet::from([owners["beta"].clone()]);
    let affected = reverse_closure(&base.edges, &[], &removed);
    assert!(
        affected.contains("candidate-renamed-alpha@2"),
        "removed old edge reaches candidate consumer"
    );
    assert!(!affected.contains("candidate-gamma"));
    Ok(())
}

#[test]
fn patches_and_cargo_configuration_broaden_without_resolution() -> Result<(), String> {
    let fixture = fixture()?;
    let root = fixture.path();
    let manifest =
        fs::read_to_string(root.join("Cargo.toml")).map_err(|error| error.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        format!("{manifest}[patch.crates-io]\npatched = {{ path = 'beta' }}\n"),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        qualify_resolution(root, &manifest_set()),
        Err("manifest_graph_requires_resolution".to_owned())
    );
    fs::write(root.join("Cargo.toml"), manifest).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join(".cargo")).map_err(|error| error.to_string())?;
    fs::write(
        root.join(".cargo/config.toml"),
        "[patch.crates-io]\npatched = { path = '../beta' }\n",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        qualify_resolution(root, &manifest_set()),
        Err("cargo_config_requires_resolution".to_owned())
    );
    Ok(())
}

#[test]
fn ancestor_cargo_configuration_is_unknown_before_metadata() -> Result<(), String> {
    let fixture = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = fixture.path().join("project");
    fs::create_dir_all(root.join("src")).map_err(|error| error.to_string())?;
    fs::write(root.join("src/lib.rs"), "").map_err(|error| error.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'child'\nversion = '0.1.0'\n",
    )
    .map_err(|error| error.to_string())?;
    fs::create_dir_all(fixture.path().join(".cargo")).map_err(|error| error.to_string())?;
    let config = fixture.path().join(".cargo/config.toml");
    fs::write(
        &config,
        "[patch.crates-io]\nshared = { path = '../private' }\n",
    )
    .map_err(|error| error.to_string())?;
    let manifests = BTreeSet::from(["Cargo.toml".to_owned()]);
    assert_eq!(
        qualify_resolution(&root, &manifests),
        Err("cargo_config_requires_resolution".to_owned())
    );
    #[cfg(unix)]
    {
        fs::remove_file(&config).map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink("missing-config", &config).map_err(|error| error.to_string())?;
        assert_eq!(
            qualify_resolution(&root, &manifests),
            Err("cargo_config_requires_resolution".to_owned())
        );
    }
    Ok(())
}
