//! Detector registration cases (T08: registered, candidate-free).
use crate::support::{Outcome, TempDir};
use velnor_actions_contract::{Stack, VelnorConfig, build_index};
use velnor_actions_tofu::{STACK_ID, discover_stack_candidates};

#[test]
fn registration_matches_contract_registry() {
    assert!(VelnorConfig::REGISTERED_STACKS.contains(&STACK_ID));
    assert_eq!(STACK_ID, Stack::Tofu.id());
    assert_eq!(Stack::require_known("tofu"), Ok(Stack::Tofu));
    assert!(Stack::all().contains(&Stack::Tofu));
}

#[test]
fn stack_id_spells_tofu() {
    assert_eq!(STACK_ID, "tofu");
}

#[test]
fn registry_lists_rust_before_tofu() {
    assert_eq!(VelnorConfig::REGISTERED_STACKS, &["rust", "tofu"]);
    assert_eq!(Stack::all(), &[Stack::Rust, Stack::Tofu]);
}

#[test]
fn stack_from_id_roundtrips() {
    assert_eq!(Stack::from_id("tofu"), Some(Stack::Tofu));
    assert_eq!(Stack::from_id("rust"), Some(Stack::Rust));
    assert_eq!(Stack::from_id("cobol"), None);
}

#[test]
fn require_known_admits_both_stacks() {
    assert_eq!(Stack::require_known("rust"), Ok(Stack::Rust));
    assert_eq!(Stack::require_known("tofu"), Ok(Stack::Tofu));
    assert!(Stack::require_known("cobol").is_err());
}

#[test]
fn detector_emits_no_candidates_from_markers_alone() -> Outcome {
    let dir = TempDir::create("tofu-detect-empty")?;
    dir.write("main.tf", "resource \"null_resource\" \"demo\" {}\n")?;
    dir.write("nested/stacks.tf", "")?;
    let index = build_index(dir.path(), &[])?;
    assert!(discover_stack_candidates(&index).is_empty());
    Ok(())
}

#[test]
fn detector_ignores_native_suffix_files() -> Outcome {
    let dir = TempDir::create("tofu-detect-native")?;
    dir.write("main.tofu", "resource \"null_resource\" \"demo\" {}\n")?;
    let index = build_index(dir.path(), &[])?;
    assert!(discover_stack_candidates(&index).is_empty());
    Ok(())
}

#[test]
fn detector_ignores_json_dialect_files() -> Outcome {
    let dir = TempDir::create("tofu-detect-json")?;
    dir.write("main.tf.json", "{\"resource\":{}}\n")?;
    let index = build_index(dir.path(), &[])?;
    assert!(discover_stack_candidates(&index).is_empty());
    Ok(())
}

#[test]
fn detector_empty_on_empty_index() -> Outcome {
    let dir = TempDir::create("tofu-detect-bare")?;
    let index = build_index(dir.path(), &[])?;
    assert!(discover_stack_candidates(&index).is_empty());
    Ok(())
}

#[test]
fn detector_empty_under_exclusions() -> Outcome {
    let dir = TempDir::create("tofu-detect-excluded")?;
    dir.write("main.tf", "")?;
    dir.write("vendor/dep/stacks.tf", "")?;
    let index = build_index(dir.path(), &["vendor/**".to_owned()])?;
    assert!(discover_stack_candidates(&index).is_empty());
    Ok(())
}

#[test]
fn units_convert_to_detector_records() {
    use velnor_actions_contract::StackCandidate;
    use velnor_actions_tofu::{detected_projects_for_units, manifest_for_unit_root};
    let candidates = vec![
        StackCandidate {
            stack_id: STACK_ID.to_owned(),
            unit_root: String::new(),
        },
        StackCandidate {
            stack_id: STACK_ID.to_owned(),
            unit_root: "infra".to_owned(),
        },
    ];
    let projects = detected_projects_for_units(&candidates);
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].stack_id, "tofu");
    assert_eq!(projects[0].project_root, "");
    assert_eq!(projects[0].manifest, "");
    assert_eq!(projects[1].project_root, "infra");
    assert_eq!(projects[1].manifest, "infra");
    assert_eq!(manifest_for_unit_root(""), "");
    assert_eq!(manifest_for_unit_root("infra"), "infra");
}

#[test]
fn conversion_preserves_candidate_order() {
    use velnor_actions_contract::StackCandidate;
    use velnor_actions_tofu::detected_projects_for_units;
    let candidates = vec![
        StackCandidate {
            stack_id: STACK_ID.to_owned(),
            unit_root: "zebra".to_owned(),
        },
        StackCandidate {
            stack_id: STACK_ID.to_owned(),
            unit_root: "alpha".to_owned(),
        },
    ];
    let projects = detected_projects_for_units(&candidates);
    assert_eq!(projects[0].project_root, "zebra");
    assert_eq!(projects[1].project_root, "alpha");
}

#[test]
fn conversion_of_empty_is_empty() {
    use velnor_actions_tofu::detected_projects_for_units;
    assert!(detected_projects_for_units(&[]).is_empty());
}
