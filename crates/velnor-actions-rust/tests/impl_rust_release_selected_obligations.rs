//! Generation validates selected local versions without observed registry data.
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use velnor_actions_rust::release_graph::validate_selected_dependency_obligations;
use velnor_actions_rust::{ReleaseError, ReleaseScope, ReleaseSelection};

use crate::release_support::{DepOpt, dep, doc, graph_of, manifest, pkg, registry, select_of};
use crate::support::{Outcome, TempDir};

/// Two independently versioned selected packages with one local obligation.
fn pair(root: &Path, requirement: &str, version: &str, options: DepOpt, reverse: bool) -> String {
    let alpha_deps = if reverse {
        vec![dep(
            "beta",
            "^1",
            DepOpt {
                path: Some(manifest(root, "crates/b")),
                ..Default::default()
            },
        )]
    } else {
        vec![]
    };
    doc(
        root,
        vec!["a-id", "b-id"],
        vec![
            pkg(
                root,
                "a-id",
                "alpha",
                version,
                "crates/a/Cargo.toml",
                Value::Null,
                alpha_deps,
            ),
            pkg(
                root,
                "b-id",
                "beta",
                "1.0.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![dep(
                    "alpha",
                    requirement,
                    DepOpt {
                        path: Some(manifest(root, "crates/a")),
                        ..options
                    },
                )],
            ),
        ],
    )
}

/// Resolve both packages using their authoritative local identities.
fn selection(json: &str, root: &Path) -> Result<ReleaseSelection, ReleaseError> {
    select_of(
        json,
        root,
        &ReleaseScope::Packages(vec!["alpha".into(), "beta".into()]),
        &BTreeSet::new(),
        &[],
    )
}

#[test]
fn selected_version_mismatch_fails_even_with_published_matching_version() -> Outcome {
    let dir = TempDir::create("selected-local-mismatch")?;
    let root = dir.path().canonicalize()?;
    let json = pair(&root, "^1", "2.0.0", DepOpt::default(), false);
    let selected = selection(&json, &root)?;
    let state = registry(&[("alpha", &["1.5.0"])]);
    for result in [
        validate_selected_dependency_obligations(&selected),
        graph_of(&selected, &json, &root, &state).map(|_| ()),
    ] {
        assert!(matches!(result, Err(ReleaseError::RequirementMismatch {
            package, dep, req, found,
        }) if package == "beta" && dep == "alpha" && req == "^1" && found == "2.0.0"));
    }
    Ok(())
}

#[test]
fn selected_requirement_uses_existing_range_and_prerelease_semantics() -> Outcome {
    let dir = TempDir::create("selected-local-ranges")?;
    let root = dir.path().canonicalize()?;
    for (requirement, version) in [
        (">=1.2, <2", "1.9.0"),
        ("~1.2", "1.2.8"),
        ("1.*", "1.9.1"),
        ("^0.2", "0.2.9"),
        ("=1.2.3-alpha.1", "1.2.3-alpha.1+build.2"),
    ] {
        let json = pair(&root, requirement, version, DepOpt::default(), false);
        validate_selected_dependency_obligations(&selection(&json, &root)?)?;
    }
    let json = pair(&root, "^1.2", "1.2.3-alpha.1", DepOpt::default(), false);
    assert!(matches!(
        validate_selected_dependency_obligations(&selection(&json, &root)?),
        Err(ReleaseError::RequirementMismatch { .. })
    ));
    let json = pair(&root, "not-a-version", "1.0.0", DepOpt::default(), false);
    assert!(matches!(
        validate_selected_dependency_obligations(&selection(&json, &root)?),
        Err(ReleaseError::InvalidRequirement { .. })
    ));
    Ok(())
}

#[test]
fn normal_build_optional_and_target_cycles_fail_before_freezing() -> Outcome {
    let dir = TempDir::create("selected-local-cycles")?;
    let root = dir.path().canonicalize()?;
    for options in [
        DepOpt::default(),
        DepOpt {
            kind: Some("build".into()),
            ..Default::default()
        },
        DepOpt {
            optional: true,
            ..Default::default()
        },
        DepOpt {
            target: Some("cfg(unix)".into()),
            ..Default::default()
        },
    ] {
        let json = pair(&root, "^1", "1.0.0", options, true);
        assert!(
            matches!(validate_selected_dependency_obligations(&selection(&json, &root)?),
            Err(ReleaseError::PublishCycle { members }) if members == ["alpha", "beta"])
        );
    }
    Ok(())
}

#[test]
fn dev_edges_validate_versions_without_creating_publication_cycles() -> Outcome {
    let dir = TempDir::create("selected-local-dev")?;
    let root = dir.path().canonicalize()?;
    let options = DepOpt {
        kind: Some("dev".into()),
        ..Default::default()
    };
    let json = pair(&root, "^1", "1.0.0", options, true);
    validate_selected_dependency_obligations(&selection(&json, &root)?)?;
    let options = DepOpt {
        kind: Some("dev".into()),
        ..Default::default()
    };
    let json = pair(&root, "^2", "1.0.0", options, false);
    assert!(matches!(
        validate_selected_dependency_obligations(&selection(&json, &root)?),
        Err(ReleaseError::RequirementMismatch { .. })
    ));
    Ok(())
}

#[test]
fn unselected_dependency_remains_an_anonymous_packaging_obligation() -> Outcome {
    let dir = TempDir::create("selected-local-unselected")?;
    let root = dir.path().canonicalize()?;
    let json = pair(&root, "^1", "1.0.0", DepOpt::default(), false);
    let selected = select_of(
        &json,
        &root,
        &ReleaseScope::Packages(vec!["beta".into()]),
        &BTreeSet::new(),
        &[],
    )?;
    validate_selected_dependency_obligations(&selected)?;
    assert_eq!(selected.names(), vec!["beta"]);
    assert!(matches!(
        graph_of(&selected, &json, &root, &registry(&[])),
        Err(ReleaseError::UnpublishedLocalDep { .. })
    ));
    Ok(())
}
