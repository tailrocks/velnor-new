//! Calling-root selection over the base/head module-graph union.
use std::collections::BTreeSet;

use velnor_actions_tofu::select::{RootSelection, SelectAllReason, select_roots};
use velnor_actions_tofu_core::modules::{ModuleEdge, ModuleEdges, ModuleFinding, SourceClass};

/// One local edge.
fn edge(from: &str, to: &str) -> ModuleEdge {
    ModuleEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        source: String::new(),
    }
}

/// Edge set from `(from, to)` pairs.
fn edges(pairs: &[(&str, &str)]) -> ModuleEdges {
    ModuleEdges {
        edges: pairs.iter().map(|(from, to)| edge(from, to)).collect(),
        findings: Vec::new(),
    }
}

/// Finding set from `(file, name, class, detail)` entries.
fn findings(entries: &[(&str, &str, SourceClass, &str)]) -> ModuleEdges {
    ModuleEdges {
        edges: Vec::new(),
        findings: entries
            .iter()
            .map(|(file, name, class, detail)| ModuleFinding {
                file: (*file).to_owned(),
                name: (*name).to_owned(),
                class: class.clone(),
                detail: (*detail).to_owned(),
            })
            .collect(),
    }
}

/// Roots list from names.
fn roots(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Changed-file set from paths.
fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// Dynamic finding over `file`.
fn dynamic(file: &str) -> ModuleEdges {
    findings(&[(file, "m", SourceClass::Dynamic, "template_source")])
}

/// Assert narrow selection equals `want` with no fallback.
fn assert_narrow(selection: &RootSelection, want: &[&str]) {
    let want: BTreeSet<String> = want.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(selection.selected, want);
    assert!(selection.fallback.is_empty(), "{:?}", selection.fallback);
}

#[test]
fn narrow_change_selects_calling_root() {
    let head = edges(&[("a", "m")]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn transitive_callers_selected() {
    let head = edges(&[("a", "m"), ("m", "n")]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &empty,
        &head,
        &changed(&["n/deep.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn removed_edge_keeps_caller_selected() {
    let base = edges(&[("a", "m")]);
    let head = edges(&[]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &base,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn delete_module_caller_kept() {
    // Head deleted `m` and its call; the deleted paths still select the caller.
    let base = edges(&[("a", "m")]);
    let head = edges(&[]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &base,
        &head,
        &changed(&["m/main.tf", "a/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn rename_module_keeps_caller() {
    let base = edges(&[("a", "m")]);
    let head = edges(&[("a", "m2")]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &base,
        &head,
        &changed(&["m/main.tf", "m2/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn delete_root_keeps_caller_selected() {
    // Root `x` doubles as `r1`'s module; deleting `x` (base root) plus
    // the call still selects the surviving caller via the base edge.
    let base = edges(&[("r1", "x")]);
    let head = edges(&[]);
    let selection = select_roots(
        &roots(&["r1"]),
        &roots(&["r1", "x"]),
        &base,
        &head,
        &changed(&["x/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["r1"]);
}

#[test]
fn child_dirs_never_auto_promoted() {
    // `m2 -> m` with no root caller: the change selects nothing, and
    // neither module dir is ever emitted as a root.
    let head = edges(&[("m2", "m")]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &empty,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &[]);
}

#[test]
fn direct_root_file_selects_its_root() {
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &empty,
        &changed(&["b/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["b"]);
}

#[test]
fn unowned_file_selects_all_with_unknown_reason() {
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &empty,
        &changed(&["stray/x.tf"]),
    )
    .expect("selects");
    assert_eq!(selection.selected, changed(&["a", "b"]));
    assert_eq!(selection.fallback.len(), 1);
    assert!(matches!(
        selection.fallback.iter().next(),
        Some(SelectAllReason::Unknown { .. })
    ));
}

#[test]
fn dynamic_finding_selects_all() {
    let head = dynamic("a/main.tf");
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&["a/main.tf"]),
    )
    .expect("selects");
    assert_eq!(selection.selected, changed(&["a", "b"]));
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::Dynamic { .. })),
        "{:?}",
        selection.fallback
    );
}

#[test]
fn external_finding_selects_all() {
    let head = findings(&[("a/main.tf", "m", SourceClass::External, "/opt/copy")]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&["a/main.tf"]),
    )
    .expect("selects");
    assert_eq!(selection.selected, changed(&["a", "b"]));
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::External { .. })),
        "{:?}",
        selection.fallback
    );
}

#[test]
fn base_only_finding_still_widens() {
    // The base closure is unknown even when head cleaned up.
    let base = dynamic("a/main.tf");
    let head = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &base,
        &head,
        &changed(&["a/main.tf"]),
    )
    .expect("selects");
    assert_eq!(selection.selected, changed(&["a", "b"]));
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::Dynamic { .. })),
        "{:?}",
        selection.fallback
    );
}

#[test]
fn remote_finding_never_widens() {
    use velnor_actions_tofu_core::modules::RemoteKind;
    let head = findings(&[(
        "a/main.tf",
        "m",
        SourceClass::Remote(RemoteKind::Registry),
        "ns/name/sys",
    )]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&["b/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["b"]);
}

#[test]
fn head_cycle_errors() {
    let head = edges(&[("a", "m"), ("m", "a")]);
    let empty = edges(&[]);
    let err = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &empty,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect_err("head cycle errors");
    assert!(err.to_string().contains("module_cycle"), "{err}");
}

#[test]
fn base_cycle_ignored_as_history() {
    // A base-only cycle (fixed at head) must not block selection.
    let base = edges(&[("a", "m"), ("m", "a")]);
    let head = edges(&[]);
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a"]),
        &base,
        &head,
        &changed(&["a/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn empty_changed_selects_nothing_despite_findings() {
    let head = dynamic("a/main.tf");
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&[]),
    )
    .expect("selects");
    assert_narrow(&selection, &[]);
}

#[test]
fn auto_tfvars_changes_select_their_owner() {
    let head = edges(&[("a", "m")]);
    let empty = edges(&[]);
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &empty,
        &head,
        &changed(&["a/terraform.tfvars", "m/extra.auto.tfvars"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn reasons_render_stable_tags() {
    assert_eq!(
        SelectAllReason::Unknown {
            detail: "p".to_owned()
        }
        .to_string(),
        "unknown:p"
    );
    assert_eq!(
        SelectAllReason::Dynamic {
            detail: "d".to_owned()
        }
        .to_string(),
        "dynamic_source:d"
    );
    assert_eq!(
        SelectAllReason::External {
            detail: "e".to_owned()
        }
        .to_string(),
        "external_source:e"
    );
}
