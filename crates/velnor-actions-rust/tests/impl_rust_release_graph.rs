//! Publication-graph cases (deps, dev edges, registries, cycles).
use std::collections::BTreeSet;

use serde_json::Value;

use crate::release_support::{
    DepOpt, dep, doc, graph_of, manifest, pkg, registry, root_of, select_of,
};
use crate::support::{Outcome, TempDir};
use velnor_actions_rust::{DepKind, ReleaseError, ReleaseScope};

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
    assert_eq!(graph.edges, [] as [velnor_actions_rust::PackagingEdge; 0]);
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
