use super::*;

/// Malformed units keep the machine-readable token byte-identical.
#[test]
fn malformed_unit_keeps_wire_token() {
    let err = DetectError::MalformedUnit {
        unit: "Cargo.toml".to_owned(),
        diagnostic: "bad toml".to_owned(),
    };
    assert_eq!(err.to_string(), "malformed_manifest:Cargo.toml: bad toml");
    let err = DetectError::DuplicateProject {
        stack_id: "rust".to_owned(),
        project_root: String::new(),
    };
    assert_eq!(err.to_string(), "duplicate_project:rust:");
}

/// Ignores apply after detection; selection skips ignored records.
#[test]
fn ignores_retain_with_reason() {
    let projects = vec![
        DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
        DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: "crates/a".to_owned(),
            manifest: "crates/a/Cargo.toml".to_owned(),
        },
    ];
    assert!(check_duplicates(&projects).is_ok());
    assert!(check_duplicates(&projects[..1]).is_ok());
    let doubled = vec![projects[0].clone(), projects[0].clone()];
    assert!(check_duplicates(&doubled).is_err());
    let statuses = apply_stack_ignores(projects, &["rust".to_owned()]);
    assert_eq!(selected_projects(&statuses).len(), 0);
    assert!(statuses.iter().all(|status| matches!(
        status,
        DetectionStatus::Ignored { reason, .. } if reason == IGNORED_REASON
    )));
}

/// The generic closure matches the local-edge behavior over pairs.
#[test]
fn closure_selects_base_and_head_consumers() {
    let base = vec![("a".to_owned(), "b".to_owned())];
    let head: Vec<(String, String)> = Vec::new();
    let changed = BTreeSet::from(["b".to_owned()]);
    assert_eq!(
        reverse_closure(&base, &head, &changed),
        BTreeSet::from(["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(
        reverse_closure(&head, &head, &changed),
        BTreeSet::from(["b".to_owned()])
    );
}
