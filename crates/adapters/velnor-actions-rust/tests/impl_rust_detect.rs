//! Detector registry cases.
use crate::support::{Outcome, TempDir};
use velnor_actions_contract::Stack;
use velnor_actions_contract_config::VelnorConfig;
use velnor_actions_contract_planning::{
    CandidateOutcome, DetectError, DetectedProject, DetectionStatus, IGNORED_REASON,
    apply_stack_ignores, build_index, check_candidate_outcomes, check_duplicates,
    selected_projects,
};
use velnor_actions_rust::{
    STACK_ID, detected_projects_for_units, discover_candidates, discover_stack_candidates,
    project_root_for_manifest,
};

#[test]
fn registry_orders_mise_rust_and_tofu() {
    assert_eq!(VelnorConfig::REGISTERED_STACKS, &["mise", "rust", "tofu"]);
    assert_eq!(Stack::all(), &[Stack::Mise, Stack::Rust, Stack::Tofu]);
}

#[test]
fn registration_matches_contract_registry() {
    assert!(VelnorConfig::REGISTERED_STACKS.contains(&STACK_ID));
    assert_eq!(STACK_ID, Stack::Rust.id());
    assert_eq!(VelnorConfig::REGISTERED_STACKS[1], STACK_ID);
}

#[test]
fn discovers_standalone_nested_and_multiple_workspaces_exactly_once() -> Outcome {
    let dir = TempDir::create("detect-multi")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    dir.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\n")?;
    dir.write("nested/inner/Cargo.toml", "[package]\nname = \"inner\"\n")?;
    dir.write("tools/Cargo.toml", "[package]\nname = \"tools\"\n")?;
    dir.write("crates/a/src/lib.rs", "")?;
    let index = build_index(dir.path(), &[])?;
    let candidates = discover_candidates(&index);
    let manifests: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.manifest.as_str())
        .collect();
    assert_eq!(
        manifests,
        vec![
            "Cargo.toml",
            "crates/a/Cargo.toml",
            "nested/inner/Cargo.toml",
            "tools/Cargo.toml",
        ]
    );
    let projects = detected_projects_for_units(&discover_stack_candidates(&index));
    let roots: Vec<&str> = projects
        .iter()
        .map(|project| project.project_root.as_str())
        .collect();
    assert_eq!(roots, vec!["", "crates/a", "nested/inner", "tools"]);
    assert!(projects.iter().all(|project| project.stack_id == "rust"));
    assert_eq!(check_duplicates(&projects), Ok(()));
    assert_eq!(project_root_for_manifest("Cargo.toml"), "");
    assert_eq!(project_root_for_manifest("crates/a/Cargo.toml"), "crates/a");
    Ok(())
}

#[test]
fn exclusions_apply_before_detection() -> Outcome {
    let dir = TempDir::create("detect-exclude")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    dir.write("vendor/dep/Cargo.toml", "[package]\n")?;
    let index = build_index(dir.path(), &["vendor/**".to_owned()])?;
    let candidates = discover_candidates(&index);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].manifest, "Cargo.toml");
    Ok(())
}

#[test]
fn ignores_apply_after_detection_with_reason() -> Outcome {
    let dir = TempDir::create("detect-ignore")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    dir.write("other/Cargo.toml", "[package]\n")?;
    let index = build_index(dir.path(), &[])?;
    let projects = detected_projects_for_units(&discover_stack_candidates(&index));
    assert_eq!(projects.len(), 2);
    let ignored = apply_stack_ignores(projects.clone(), &["rust".to_owned()]);
    assert_eq!(ignored.len(), 2);
    for status in &ignored {
        let DetectionStatus::Ignored { project, reason } = status else {
            return Err("ignored detection must carry ignored status".into());
        };
        assert_eq!(reason, IGNORED_REASON);
        assert_eq!(project.stack_id, "rust");
    }
    assert!(selected_projects(&ignored).is_empty());
    let selected = apply_stack_ignores(projects, &[]);
    assert_eq!(selected_projects(&selected).len(), 2);
    Ok(())
}

#[test]
fn duplicate_project_root_rejected() {
    let project = DetectedProject {
        stack_id: "rust".to_owned(),
        project_root: String::new(),
        manifest: "Cargo.toml".to_owned(),
    };
    let duplicated = vec![project.clone(), project];
    assert!(matches!(
        check_duplicates(&duplicated),
        Err(DetectError::DuplicateProject { .. })
    ));
    let distinct = vec![
        DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
        DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: "other".to_owned(),
            manifest: "other/Cargo.toml".to_owned(),
        },
    ];
    assert_eq!(check_duplicates(&distinct), Ok(()));
}

#[test]
fn ignored_malformed_still_reports_error() -> Outcome {
    let dir = TempDir::create("detect-malformed")?;
    dir.write("Cargo.toml", "not toml [[[\n")?;
    let index = build_index(dir.path(), &[])?;
    let projects = detected_projects_for_units(&discover_stack_candidates(&index));
    let statuses = apply_stack_ignores(projects, &["rust".to_owned()]);
    let outcomes = vec![CandidateOutcome {
        manifest: "Cargo.toml".to_owned(),
        metadata_ok: false,
        diagnostic: Some("expected expression".to_owned()),
    }];
    let result = check_candidate_outcomes(statuses, &outcomes);
    assert!(matches!(result, Err(DetectError::MalformedUnit { .. })));
    if let Err(DetectError::MalformedUnit { unit, diagnostic }) = result {
        assert_eq!(unit, "Cargo.toml");
        assert_eq!(diagnostic, "expected expression");
    }
    let ok = vec![CandidateOutcome {
        manifest: "Cargo.toml".to_owned(),
        metadata_ok: true,
        diagnostic: None,
    }];
    let index = build_index(dir.path(), &[])?;
    let projects = detected_projects_for_units(&discover_stack_candidates(&index));
    let statuses = apply_stack_ignores(projects, &[]);
    assert_eq!(check_candidate_outcomes(statuses, &ok)?.len(), 1);
    Ok(())
}
