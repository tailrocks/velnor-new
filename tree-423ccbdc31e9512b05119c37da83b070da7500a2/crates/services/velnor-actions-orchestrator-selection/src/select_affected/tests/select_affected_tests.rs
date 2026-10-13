use super::*;

#[test]
fn root_package_does_not_classify_stray_files() {
    let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
    let stray: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
    assert!(
        has_unowned_file(&discovery, &stray),
        "root prefix must not mask stray files"
    );
    let nested: BTreeSet<String> = ["crates/a/src/lib.rs".to_owned()].into_iter().collect();
    assert!(
        !has_unowned_file(&discovery, &nested),
        "nested paths stay classified"
    );
}

#[test]
fn lone_root_package_classifies_everything() {
    let discovery = discovery_with(&[("root", "Cargo.toml")]);
    let changed: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
    assert!(
        !has_unowned_file(&discovery, &changed),
        "one package selects itself either way"
    );
    let selected = affected_packages(&discovery, &changed, &[], &[]);
    assert_eq!(selected, ["root".to_owned()].into_iter().collect());
}

#[test]
fn nested_change_selects_deepest_beside_root() {
    let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
    let changed: BTreeSet<String> = ["crates/a/src/lib.rs".to_owned()].into_iter().collect();
    let selected = affected_packages(&discovery, &changed, &[], &[]);
    assert_eq!(selected, ["a".to_owned()].into_iter().collect());
}

#[test]
fn shared_declared_input_selects_every_declarer() {
    use velnor_actions_rust::{TaskGroup, TaskKind};
    let mut discovery = two_package_discovery();
    let group = |package: &str| TaskGroup {
        task_id: format!("stack/rust/{package}/clippy/default"),
        package_id: package.to_owned(),
        package_name: package.to_owned(),
        manifest_key: package.to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: vec!["docs/shared.md".to_owned()],
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    };
    let mut proposals = Vec::new();
    for package in ["a", "b"] {
        let task = velnor_actions_rust::propose_task(&group(package)).expect("fixture proposes");
        task.validate().expect("fixture valid");
        proposals.push(task);
    }
    discovery.proposals = proposals;
    let changed: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
    let selected = affected_packages(&discovery, &changed, &[], &[]);
    assert!(
        selected.contains("a") && selected.contains("b"),
        "both declarers affected: {selected:?}"
    );
}

#[test]
fn union_of_base_and_head_graphs_selects_removed_consumers() {
    let discovery = two_package_discovery();
    let changed: BTreeSet<String> = ["b/src/lib.rs".to_owned()].into_iter().collect();
    let base = vec![edge("a", "b")];
    let head: Vec<(String, String)> = Vec::new();
    let selected = affected_packages(&discovery, &changed, &base, &head);
    assert_eq!(
        selected,
        ["a".to_owned(), "b".to_owned()].into_iter().collect(),
        "base-only edge selects its consumer"
    );
    let selected = affected_packages(&discovery, &changed, &[], &head);
    assert_eq!(
        selected,
        ["b".to_owned()].into_iter().collect(),
        "without the base edge only the owner is selected"
    );
}

#[test]
fn declared_inputs_select_their_package() {
    use velnor_actions_rust::{TaskGroup, TaskKind};
    use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};
    let mut discovery = two_package_discovery();
    let group = TaskGroup {
        task_id: "stack/rust/a/clippy/default".to_owned(),
        package_id: "a".to_owned(),
        package_name: "a".to_owned(),
        manifest_key: "a".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: vec!["docs/spec.md".to_owned()],
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    discovery.proposals = vec![task];
    let changed: BTreeSet<String> = ["docs/spec.md".to_owned()].into_iter().collect();
    let selected = affected_packages(&discovery, &changed, &[], &[]);
    assert!(
        selected.contains("a"),
        "declared input selects package a: {selected:?}"
    );
}

#[test]
fn skipped_index_names_broaden_with_explicit_tag() {
    use velnor_actions_contract_workflow::WorkflowEvent;
    let mut discovery = two_package_discovery();
    discovery.skipped_non_utf8 = true;
    let mut warnings = Vec::new();
    let changed = classify_changed(
        std::path::Path::new("/nonexistent"),
        WorkflowEvent::PullRequest,
        Some("base"),
        "head",
        &discovery,
        &mut warnings,
    );
    assert_eq!(changed, None, "skipped names broaden to all");
    assert!(
        warnings.iter().any(|w| w.contains("non_utf8_path")),
        "explicit tag: {warnings:?}"
    );
}
