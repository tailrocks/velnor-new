//! Release graph: dependency order, edge kinds, and fail-closed sets.
use crate::impl_rust_release_modes::{
    DepOpt, dep, doc, graph_of, manifest, pkg, registry, root_of, select_of,
};
use crate::support::{Outcome, TempDir};
use serde_json::{Value, json};
use velnor_actions_rust_core::{DepKind, ReleaseError, ReleaseScope};

#[test]
fn dependency_chain_publishes_leaves_first() -> Outcome {
    let dir = TempDir::create("release-chain")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id", "c-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "aaa",
                "1.0.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "bbb",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![dep(
                    "aaa",
                    "^1",
                    DepOpt {
                        path: Some(manifest(&root, "crates/a")),
                        ..Default::default()
                    },
                )],
            ),
            pkg(
                &root,
                "c-id",
                "ccc",
                "1.0.0",
                "crates/c/Cargo.toml",
                Value::Null,
                vec![dep(
                    "bbb",
                    "^1",
                    DepOpt {
                        path: Some(manifest(&root, "crates/b")),
                        ..Default::default()
                    },
                )],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["ccc".to_owned(), "bbb".to_owned(), "aaa".to_owned()]);
    let selection = select_of(&json, &root, &scope, &[])?;
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(
        graph.order,
        vec!["aaa".to_owned(), "bbb".to_owned(), "ccc".to_owned()]
    );
    assert_eq!(graph.edges.len(), 2);
    Ok(())
}

#[test]
fn optional_build_and_target_edges_constrain_order() -> Outcome {
    let dir = TempDir::create("release-edgekinds")?;
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
                vec![
                    dep(
                        "alpha",
                        "^1",
                        DepOpt {
                            path: Some(manifest(&root, "crates/a")),
                            optional: true,
                            ..Default::default()
                        },
                    ),
                    dep(
                        "alpha",
                        "^1",
                        DepOpt {
                            kind: Some("build".to_owned()),
                            path: Some(manifest(&root, "crates/a")),
                            ..Default::default()
                        },
                    ),
                    dep(
                        "alpha",
                        "^1",
                        DepOpt {
                            path: Some(manifest(&root, "crates/a")),
                            target: Some("cfg(unix)".to_owned()),
                            ..Default::default()
                        },
                    ),
                ],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned(), "beta".to_owned()]);
    let selection = select_of(&json, &root, &scope, &[])?;
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(graph.order, vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(graph.edges.len(), 3);
    assert!(graph.edges.iter().any(|edge| edge.optional));
    assert!(graph.edges.iter().any(|edge| edge.kind == DepKind::Build));
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.target.as_deref() == Some("cfg(unix)"))
    );
    Ok(())
}

#[test]
fn publishable_workspace_fails_on_restricted_registries() -> Outcome {
    let dir = TempDir::create("release-restricted")?;
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
    let scope = ReleaseScope::PublishableWorkspace;
    assert!(matches!(
        select_of(&json, &root, &scope, &[]),
        Err(ReleaseError::UnsupportedRegistry { .. })
    ));
    let supported = vec!["other-reg".to_owned()];
    let selection = select_of(&json, &root, &scope, &supported)?;
    assert_eq!(selection.names(), vec!["alpha"]);
    Ok(())
}

#[test]
fn ambiguous_workspace_names_and_cycles_fail_closed() -> Outcome {
    let dir = TempDir::create("release-ambiguous")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "same",
                "1.0.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "same",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["same".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &[]),
        Err(ReleaseError::AmbiguousPackageName { .. })
    ));
    let cycle = doc(
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
                vec![dep(
                    "beta",
                    "^1",
                    DepOpt {
                        path: Some(manifest(&root, "crates/b")),
                        ..Default::default()
                    },
                )],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![dep(
                    "alpha",
                    "^1",
                    DepOpt {
                        path: Some(manifest(&root, "crates/a")),
                        ..Default::default()
                    },
                )],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned(), "beta".to_owned()]);
    let selection = select_of(&cycle, &root, &scope, &[])?;
    let Err(ReleaseError::PublishCycle { members }) =
        graph_of(&selection, &cycle, &root, &registry(&[]))
    else {
        return Err("publish cycle must fail".into());
    };
    assert_eq!(members, vec!["alpha".to_owned(), "beta".to_owned()]);
    Ok(())
}
