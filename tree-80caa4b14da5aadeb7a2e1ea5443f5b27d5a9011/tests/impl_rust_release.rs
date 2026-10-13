//! Release-set selection, publication graph, and config emission cases.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::{Outcome, TempDir};
use velnor_actions_rust::{
    DEFAULT_TAG_PATTERN, DepKind, EmitOptions, ExistingTag, PublicationGraph, RegistryState,
    ReleaseError, ReleaseRequest, ReleaseScope, ReleaseSelection, ResolvedScope, TagOutcome,
    TagState, VersionGroup, classify_tag, emit_bootstrap_config, emit_release_plz_config,
    parse_metadata_json, publication_graph, resolve_version_groups, select_release_set,
};

/// Absolute manifest path text for `relative` under `root`.
fn manifest(root: &Path, relative: &str) -> String {
    root.join(relative).to_string_lossy().into_owned()
}

/// Minimal package value; `publish` is raw JSON (null or an array).
fn pkg(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    rel: &str,
    publish: Value,
    deps: Vec<Value>,
) -> Value {
    let path = manifest(root, rel);
    json!({
        "name": name, "version": version, "id": id, "manifest_path": path,
        "publish": publish,
        "targets": [{"kind": ["lib"], "name": name, "test": true,
                     "doctest": true, "required_features": []}],
        "features": {},
        "dependencies": deps,
    })
}

/// Dependency knobs (defaults describe a normal registry dependency).
#[derive(Default)]
struct DepOpt {
    kind: Option<String>,
    optional: bool,
    target: Option<String>,
    source: Option<String>,
    registry: Option<String>,
    path: Option<String>,
}

/// One dependency declaration value.
fn dep(name: &str, req: &str, opt: DepOpt) -> Value {
    let source = opt.source.or_else(|| {
        if opt.path.is_none() {
            Some("registry+https://example.invalid/index".to_owned())
        } else {
            None
        }
    });
    json!({
        "name": name, "req": req,
        "kind": opt.kind, "optional": opt.optional, "target": opt.target,
        "source": source, "registry": opt.registry, "path": opt.path,
    })
}

/// Minimal `cargo metadata` document text.
fn doc(root: &Path, members: Vec<&str>, packages: Vec<Value>) -> String {
    let root_text = root.to_string_lossy().into_owned();
    json!({
        "version": 1, "workspace_root": root_text,
        "workspace_members": members, "packages": packages,
    })
    .to_string()
}

/// Root path helper (canonicalized temp dir).
fn root_of(dir: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(dir.path().canonicalize()?)
}

/// Run selection with release enabled.
fn select_of(
    json: &str,
    root: &Path,
    scope: &ReleaseScope,
    affected: &BTreeSet<String>,
    supported: &[String],
) -> Result<ReleaseSelection, ReleaseError> {
    select_release_set(&ReleaseRequest {
        metadata_json: json,
        repo_root: root,
        manifest_hint: "Cargo.toml",
        scope,
        enabled: true,
        affected,
        supported_registries: supported,
    })
}

/// Observed registry state from name-to-versions pairs.
fn registry(entries: &[(&str, &[&str])]) -> RegistryState {
    let published = entries
        .iter()
        .map(|(name, versions)| {
            (
                (*name).to_owned(),
                versions.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    RegistryState { published }
}

/// Build the publication graph with default supported registries.
fn graph_of(
    selection: &ReleaseSelection,
    json: &str,
    root: &Path,
    state: &RegistryState,
) -> Result<PublicationGraph, ReleaseError> {
    let record = parse_metadata_json(json, root, "Cargo.toml").map_err(ReleaseError::Metadata)?;
    publication_graph(selection, &record, state, &[])
}

#[test]
fn disabled_release_short_circuits_without_parsing() -> Outcome {
    let dir = TempDir::create("release-disabled")?;
    let root = root_of(&dir)?;
    let scope = ReleaseScope::PublishableWorkspace;
    let affected = BTreeSet::new();
    let selection = select_release_set(&ReleaseRequest {
        metadata_json: "not json",
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: false,
        affected: &affected,
        supported_registries: &[],
    })?;
    assert!(selection.is_empty());
    assert_eq!(selection.scope, ResolvedScope::Disabled);
    let options = EmitOptions {
        tag_pattern: DEFAULT_TAG_PATTERN,
        groups: &[],
    };
    assert!(matches!(
        emit_release_plz_config(&selection, &options),
        Err(ReleaseError::NothingSelected)
    ));
    Ok(())
}

#[test]
fn single_root_package_selects_with_default_registry() -> Outcome {
    let dir = TempDir::create("release-single")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["solo-id"],
        vec![pkg(
            &root,
            "solo-id",
            "solo",
            "1.4.2",
            "Cargo.toml",
            Value::Null,
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["solo".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert_eq!(selection.names(), vec!["solo"]);
    assert_eq!(
        selection.packages[0].registries,
        vec!["crates-io".to_owned()]
    );
    assert_eq!(selection.packages[0].manifest, "Cargo.toml");
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(graph.order, vec!["solo".to_owned()]);
    assert!(graph.edges.is_empty());
    Ok(())
}

#[test]
fn explicit_subset_ignores_ci_affected_state() -> Outcome {
    let dir = TempDir::create("release-subset")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id", "c-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "0.2.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "0.3.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "c-id",
                "gamma",
                "0.4.0",
                "crates/c/Cargo.toml",
                Value::Null,
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["gamma".to_owned(), "alpha".to_owned()]);
    let affected = BTreeSet::from(["b-id".to_owned(), "ghost-id".to_owned()]);
    let selection = select_of(&json, &root, &scope, &affected, &[])?;
    assert_eq!(selection.names(), vec!["alpha", "gamma"]);
    assert_eq!(selection.scope, ResolvedScope::Explicit);
    Ok(())
}

#[test]
fn publishable_workspace_skips_private_helpers() -> Outcome {
    let dir = TempDir::create("release-workspace")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["app-id", "help-id"],
        vec![
            pkg(
                &root,
                "app-id",
                "app",
                "2.0.0",
                "crates/app/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "help-id",
                "helper",
                "2.0.0",
                "crates/helper/Cargo.toml",
                json!([]),
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::PublishableWorkspace;
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert_eq!(selection.names(), vec!["app"]);
    let explicit = ReleaseScope::Packages(vec!["helper".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &explicit, &BTreeSet::new(), &[]),
        Err(ReleaseError::PublishForbidden { .. })
    ));
    Ok(())
}

#[test]
fn unsupported_registry_fails_closed_until_supported() -> Outcome {
    let dir = TempDir::create("release-registry")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id"],
        vec![pkg(
            &root,
            "a-id",
            "alpha",
            "0.2.0",
            "Cargo.toml",
            json!(["other-reg"]),
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::UnsupportedRegistry { .. })
    ));
    let supported = vec!["other-reg".to_owned()];
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &supported)?;
    assert_eq!(
        selection.packages[0].registries,
        vec!["other-reg".to_owned()]
    );
    Ok(())
}

#[test]
fn unknown_duplicate_invalid_and_external_selection_fail() -> Outcome {
    let dir = TempDir::create("release-names")?;
    let root = root_of(&dir)?;
    let outside = TempDir::create("release-outside")?;
    let ext_manifest = outside
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();
    let mut external = pkg(
        &root,
        "a-id",
        "alpha",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    let mut ext_pkg = pkg(
        &root,
        "ext-id",
        "external",
        "1.0.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    ext_pkg["manifest_path"] = Value::String(ext_manifest);
    external["manifest_path"] = Value::String(manifest(&root, "Cargo.toml"));
    let json = doc(&root, vec!["a-id"], vec![external, ext_pkg]);
    let affected = BTreeSet::new();
    for (names, check) in [
        (vec!["ghost"], "unknown"),
        (vec!["alpha", "alpha"], "duplicate"),
        (vec!["bad name!"], "invalid"),
        (vec!["external"], "external"),
    ] {
        let scope = ReleaseScope::Packages(names.iter().map(ToString::to_string).collect());
        let result = select_of(&json, &root, &scope, &affected, &[]);
        let ok = match check {
            "unknown" => matches!(result, Err(ReleaseError::UnknownPackage { .. })),
            "duplicate" => matches!(result, Err(ReleaseError::DuplicateSelection { .. })),
            "invalid" => matches!(result, Err(ReleaseError::InvalidSelectionName { .. })),
            _ => matches!(result, Err(ReleaseError::NotWorkspaceMember { .. })),
        };
        assert!(ok, "selection {names:?} must fail closed as {check}");
    }
    Ok(())
}

#[test]
fn escaping_and_outside_manifests_fail_closed() -> Outcome {
    let dir = TempDir::create("release-paths")?;
    let root = root_of(&dir)?;
    let sneaky = format!("{}/crates/../sneaky/Cargo.toml", root.to_string_lossy());
    let mut escaped = pkg(
        &root,
        "a-id",
        "alpha",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    escaped["manifest_path"] = Value::String(sneaky);
    let json = doc(&root, vec!["a-id"], vec![escaped]);
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::ManifestEscape { .. })
    ));
    let outside = TempDir::create("release-absent")?;
    let abs_manifest = outside
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();
    let mut outside_pkg = pkg(
        &root,
        "b-id",
        "beta",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    outside_pkg["manifest_path"] = Value::String(abs_manifest);
    let json = doc(&root, vec!["b-id"], vec![outside_pkg]);
    let scope = ReleaseScope::Packages(vec!["beta".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::ManifestOutsideRoot { .. })
    ));
    Ok(())
}

#[test]
fn unparseable_and_unsatisfied_requirements_fail() -> Outcome {
    let dir = TempDir::create("release-reqs")?;
    let root = root_of(&dir)?;
    let alpha = pkg(
        &root,
        "a-id",
        "alpha",
        "1.0.0",
        "crates/a/Cargo.toml",
        Value::Null,
        vec![],
    );
    let bad_dep = dep(
        "alpha",
        "bogus!!",
        DepOpt {
            path: Some(manifest(&root, "crates/a")),
            ..Default::default()
        },
    );
    let beta = pkg(
        &root,
        "b-id",
        "beta",
        "1.0.0",
        "crates/b/Cargo.toml",
        Value::Null,
        vec![bad_dep],
    );
    let json = doc(&root, vec!["a-id", "b-id"], vec![alpha, beta]);
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned(), "beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert!(matches!(
        graph_of(&selection, &json, &root, &registry(&[])),
        Err(ReleaseError::InvalidRequirement { .. })
    ));
    let mismatch_dep = dep(
        "alpha",
        "^9",
        DepOpt {
            path: Some(manifest(&root, "crates/a")),
            ..Default::default()
        },
    );
    let beta = pkg(
        &root,
        "b-id",
        "beta",
        "1.0.0",
        "crates/b/Cargo.toml",
        Value::Null,
        vec![mismatch_dep],
    );
    let alpha = pkg(
        &root,
        "a-id",
        "alpha",
        "1.0.0",
        "crates/a/Cargo.toml",
        Value::Null,
        vec![],
    );
    let json = doc(&root, vec!["a-id", "b-id"], vec![alpha, beta]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert!(matches!(
        graph_of(&selection, &json, &root, &registry(&[])),
        Err(ReleaseError::RequirementMismatch { .. })
    ));
    Ok(())
}

#[test]
fn unpublished_local_dep_names_fix_or_uses_registry() -> Outcome {
    let dir = TempDir::create("release-missing")?;
    let root = root_of(&dir)?;
    let need = dep(
        "alpha",
        "^1",
        DepOpt {
            path: Some(manifest(&root, "crates/a")),
            ..Default::default()
        },
    );
    let json = doc(
        &root,
        vec!["a-id", "b-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "1.2.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![need],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    let Err(error) = graph_of(&selection, &json, &root, &registry(&[])) else {
        return Err("unpublished local dep must fail".into());
    };
    let text = error.to_string();
    assert!(
        text.contains("unpublished_local_dep:beta"),
        "unexpected: {text}"
    );
    assert!(
        text.contains("add \"alpha\" to release.packages"),
        "unexpected: {text}"
    );
    let published = registry(&[("alpha", &["1.2.0"])]);
    let graph = graph_of(&selection, &json, &root, &published)?;
    assert_eq!(graph.order, vec!["beta".to_owned()]);
    assert!(graph.edges.is_empty());
    Ok(())
}

#[test]
fn dev_edges_cannot_invent_publish_cycles() -> Outcome {
    let dir = TempDir::create("release-dev")?;
    let root = root_of(&dir)?;
    let a_path = manifest(&root, "crates/a");
    let b_path = manifest(&root, "crates/b");
    let dev_on_b = dep(
        "beta",
        "^1",
        DepOpt {
            kind: Some("dev".to_owned()),
            path: Some(b_path),
            ..Default::default()
        },
    );
    let use_a = dep(
        "alpha",
        "^1",
        DepOpt {
            path: Some(a_path),
            ..Default::default()
        },
    );
    let json = doc(
        &root,
        vec!["a-id", "b-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "1.0.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![dev_on_b],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![use_a],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned(), "beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(graph.order, vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].kind, DepKind::Normal);
    Ok(())
}

#[test]
fn registry_deps_need_published_satisfying_versions() -> Outcome {
    let dir = TempDir::create("release-regdep")?;
    let root = root_of(&dir)?;
    let need = dep("serde", "^1", DepOpt::default());
    let json = doc(
        &root,
        vec!["b-id"],
        vec![pkg(
            &root,
            "b-id",
            "beta",
            "1.0.0",
            "Cargo.toml",
            Value::Null,
            vec![need],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert!(graph_of(&selection, &json, &root, &registry(&[])).is_err());
    let stale = registry(&[("serde", &["0.9.0"])]);
    assert!(graph_of(&selection, &json, &root, &stale).is_err());
    let fresh = registry(&[("serde", &["1.0.0"])]);
    let graph = graph_of(&selection, &json, &root, &fresh)?;
    assert_eq!(graph.order, vec!["beta".to_owned()]);
    Ok(())
}

#[test]
fn git_only_dependencies_are_rejected() -> Outcome {
    let dir = TempDir::create("release-git")?;
    let root = root_of(&dir)?;
    let need = dep(
        "tool",
        "^1",
        DepOpt {
            source: Some("git+https://example.invalid/tool".to_owned()),
            ..Default::default()
        },
    );
    let json = doc(
        &root,
        vec!["b-id"],
        vec![pkg(
            &root,
            "b-id",
            "beta",
            "1.0.0",
            "Cargo.toml",
            Value::Null,
            vec![need],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    let Err(error) = graph_of(&selection, &json, &root, &registry(&[])) else {
        return Err("git-only dep must fail".into());
    };
    assert!(error.to_string().contains("git_only_dep:beta:tool"));
    Ok(())
}

#[test]
fn version_groups_take_max_while_unchanged_stay() -> Outcome {
    let groups = vec![VersionGroup {
        name: "core".to_owned(),
        members: vec!["alpha".to_owned(), "beta".to_owned()],
    }];
    let current = BTreeMap::from([
        ("alpha".to_owned(), "1.0.0".to_owned()),
        ("beta".to_owned(), "1.1.0".to_owned()),
    ]);
    let planned = BTreeMap::from([
        ("alpha".to_owned(), "1.1.0".to_owned()),
        ("beta".to_owned(), "1.2.0".to_owned()),
    ]);
    let changed = BTreeSet::from(["alpha".to_owned(), "beta".to_owned()]);
    let resolved = resolve_version_groups(&groups, &current, &planned, &changed)?;
    assert_eq!(resolved.get("alpha").map(String::as_str), Some("1.2.0"));
    assert_eq!(resolved.get("beta").map(String::as_str), Some("1.2.0"));
    let changed = BTreeSet::from(["alpha".to_owned()]);
    let resolved = resolve_version_groups(&groups, &current, &planned, &changed)?;
    assert_eq!(resolved.get("alpha").map(String::as_str), Some("1.1.0"));
    assert_eq!(resolved.get("beta").map(String::as_str), Some("1.1.0"));
    let overlapping = vec![
        VersionGroup {
            name: "one".to_owned(),
            members: vec!["alpha".to_owned()],
        },
        VersionGroup {
            name: "two".to_owned(),
            members: vec!["alpha".to_owned()],
        },
    ];
    assert!(matches!(
        resolve_version_groups(&overlapping, &current, &planned, &changed),
        Err(ReleaseError::OverlappingGroups { .. })
    ));
    Ok(())
}

#[test]
fn emission_is_deterministic_with_bootstrap_variant() -> Outcome {
    let dir = TempDir::create("release-emit")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "1.0.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["beta".to_owned(), "alpha".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    let groups = vec![VersionGroup {
        name: "core".to_owned(),
        members: vec!["alpha".to_owned(), "beta".to_owned()],
    }];
    let options = EmitOptions {
        tag_pattern: DEFAULT_TAG_PATTERN,
        groups: &groups,
    };
    let first = emit_release_plz_config(&selection, &options)?;
    let second = emit_release_plz_config(&selection, &options)?;
    assert_eq!(first, second);
    let expected = "# Generated by Velnor Actions; do not edit.\n\
        [workspace]\nrelease = false\nrelease_always = false\nsemver_check = true\n\
        publish_no_verify = false\npublish_allow_dirty = false\n\
        git_tag_name = \"{{ package }}-v{{ version }}\"\n\
        \n[[package]]\nname = \"alpha\"\nrelease = true\npublish = true\ngit_only = false\n\
        version_group = \"core\"\n\
        \n[[package]]\nname = \"beta\"\nrelease = true\npublish = true\ngit_only = false\n\
        version_group = \"core\"\n";
    assert_eq!(first, expected);
    let bootstrap = emit_bootstrap_config(&selection, &options)?;
    assert!(bootstrap.contains("release_always = true"));
    assert_eq!(
        bootstrap.replace("release_always = true", "release_always = false"),
        first
    );
    Ok(())
}

#[test]
fn tag_collision_registry_absence_and_repair_stay_distinct() {
    let found = ExistingTag {
        tag: "alpha-v1.0.0".to_owned(),
        sha: "b".repeat(40),
    };
    let source = "a".repeat(40);
    let ready = classify_tag(&TagState {
        expected_tag: "alpha-v1.0.0".to_owned(),
        existing: vec![],
        source_sha: source.clone(),
        registry_has_version: false,
    });
    assert_eq!(ready, TagOutcome::ReadyToPublish);
    let collision = classify_tag(&TagState {
        expected_tag: "alpha-v1.0.0".to_owned(),
        existing: vec![found.clone()],
        source_sha: source.clone(),
        registry_has_version: false,
    });
    assert!(matches!(collision, TagOutcome::TagCollision { .. }));
    let repair = classify_tag(&TagState {
        expected_tag: "alpha-v1.0.0".to_owned(),
        existing: vec![],
        source_sha: source.clone(),
        registry_has_version: true,
    });
    assert!(matches!(repair, TagOutcome::MetadataRepairNeeded { .. }));
    let released = classify_tag(&TagState {
        expected_tag: "alpha-v1.0.0".to_owned(),
        existing: vec![ExistingTag {
            tag: "alpha-v1.0.0".to_owned(),
            sha: source.clone(),
        }],
        source_sha: source,
        registry_has_version: true,
    });
    assert!(matches!(released, TagOutcome::AlreadyReleased { .. }));
}

#[test]
fn tag_pattern_requires_placeholders_and_escapes_toml() -> Outcome {
    let dir = TempDir::create("release-tag")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id"],
        vec![pkg(
            &root,
            "a-id",
            "alpha",
            "1.0.0",
            "Cargo.toml",
            Value::Null,
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    let bad = EmitOptions {
        tag_pattern: "v{{ version }}",
        groups: &[],
    };
    assert!(matches!(
        emit_release_plz_config(&selection, &bad),
        Err(ReleaseError::InvalidTagPattern { .. })
    ));
    let quoted = EmitOptions {
        tag_pattern: "{{ package }}-\"v{{ version }}\"",
        groups: &[],
    };
    let rendered = emit_release_plz_config(&selection, &quoted)?;
    assert!(rendered.contains("git_tag_name = \"{{ package }}-\\\"v{{ version }}\\\"\""));
    Ok(())
}
