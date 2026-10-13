use super::*;

fn variant(state: &str, entries: &[(&str, u64)]) -> BTreeMap<String, serde_json::Value> {
    let marker = format!("workspace/sig/state/{state}");
    let mut inventory = BTreeMap::from([
        (marker.clone(), serde_json::json!({"type":"workspace"})),
        (
            format!("{marker}/target"),
            serde_json::json!({"type":"root"}),
        ),
    ]);
    inventory.extend(entries.iter().map(|(path, digest)| {
        (
            format!("{marker}/target/{path}"),
            serde_json::json!({"type":"file", "content":digest}),
        )
    }));
    inventory
}

#[test]
fn deletion_only_workspace_change_is_reported_without_useful_work() {
    let before = variant("before", &[("a", 1), ("b", 2)]);
    let subset = variant("subset", &[("a", 1)]);
    let empty = variant("empty", &[]);
    assert_eq!(workspace_variants(&before, &subset), (0, 1, 0));
    assert_eq!(workspace_variants(&before, &empty), (0, 1, 0));
    let delta = Delta {
        changed_workspace_variants: 1,
        ..Delta::default()
    };
    assert!(!delta.useful());
}

#[test]
fn workspace_additions_and_repairs_are_useful() {
    let before = variant("before", &[("a", 1)]);
    assert_eq!(
        workspace_variants(&before, &variant("added", &[("a", 1), ("b", 2)])),
        (0, 1, 1)
    );
    assert_eq!(
        workspace_variants(&before, &variant("repaired", &[("a", 3)])),
        (0, 1, 1)
    );
    let delta = Delta {
        reusable_workspace_variants: 1,
        ..Delta::default()
    };
    assert!(delta.useful());
}

#[test]
fn workspace_subset_of_any_retained_variant_is_not_useful() {
    let mut before = variant("first", &[("a", 1)]);
    before.extend(variant("second", &[("a", 1), ("b", 2)]));
    assert_eq!(
        workspace_variants(&before, &variant("subset", &[("b", 2)])),
        (0, 1, 0)
    );
}

#[test]
fn disjoint_baseline_variants_are_not_combined_to_hide_new_work() {
    let mut before = variant("first", &[("a", 1)]);
    before.extend(variant("second", &[("b", 2)]));
    assert_eq!(
        workspace_variants(&before, &variant("combined", &[("a", 1), ("b", 2)])),
        (0, 1, 1)
    );
}

#[test]
fn semantic_mode_symlink_and_root_role_changes_are_useful() {
    let before = variant("before", &[("native-scheduler.bin", 1)]);
    let mut mode = variant("mode", &[("native-scheduler.bin", 1)]);
    mode.get_mut("workspace/sig/state/mode/target/native-scheduler.bin")
        .unwrap()["mode"] = serde_json::json!(0o755);
    assert_eq!(workspace_variants(&before, &mode), (0, 1, 1));
    let mut link = variant("link", &[]);
    link.insert(
        "workspace/sig/state/link/target/link".into(),
        serde_json::json!({"type":"symlink","target":"new", "directory":false}),
    );
    assert_eq!(workspace_variants(&before, &link), (0, 1, 1));
    let relocated_role = mode
        .into_iter()
        .map(|(key, value)| (key.replace("/target", "/build"), value))
        .collect();
    assert_eq!(workspace_variants(&before, &relocated_role), (0, 1, 1));
}

#[test]
fn new_workspace_with_only_root_markers_adds_no_reusable_work() {
    assert_eq!(
        workspace_variants(&BTreeMap::new(), &variant("empty", &[])),
        (1, 0, 0)
    );
}

#[test]
fn added_empty_role_marker_does_not_add_reusable_work() {
    let before = variant("before", &[("a", 1)]);
    let mut after = variant("after", &[("a", 1)]);
    after.insert(
        "workspace/sig/state/after/build".into(),
        serde_json::json!({"type":"root"}),
    );
    assert_eq!(workspace_variants(&before, &after), (0, 1, 0));
    let reverse = variant("reverse", &[("a", 1)]);
    assert_eq!(workspace_variants(&after, &reverse), (0, 1, 0));
}
