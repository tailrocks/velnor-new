//! T27 lane A: tofu selection/topology/impact pins (spec §10).
//!
//! Pins the REAL current behavior for six open items: provider
//! aliases, named independent roots, formatting-only changes,
//! external-template impact, missing base comparison, and
//! tool/config reselect. Conservative behavior is pinned as-is.
use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::{TofuStackConfig, Utf8RepoRelDir};
use velnor_actions_contract_planning::build_index;
use velnor_actions_tofu::select::{RootSelection, SelectAllReason, select_roots};
use velnor_actions_tofu_core::modules::{ModuleEdges, SourceClass};
use velnor_actions_tofu_core::{
    analyze_files, classify, fmt_scope_for_root, module_refs_for_texts, qualify_roots, resolve_refs,
};

use crate::support::{Outcome, TempDir};

/// Roots list from names.
fn roots(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Changed-file set from paths.
fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// Empty edge set (no edges, no findings).
fn empty() -> ModuleEdges {
    ModuleEdges {
        edges: Vec::new(),
        findings: Vec::new(),
    }
}

/// Tofu config from raw root spellings.
fn tofu(raw: &[&str]) -> TofuStackConfig {
    TofuStackConfig {
        roots: raw
            .iter()
            .map(|entry| Utf8RepoRelDir::from_raw((*entry).to_owned()))
            .collect(),
    }
}

/// Assert narrow selection equals `want` with no fallback.
fn assert_narrow(selection: &RootSelection, want: &[&str]) {
    let want: BTreeSet<String> = want.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(selection.selected, want);
    assert!(selection.fallback.is_empty(), "{:?}", selection.fallback);
}

/// Assert selection covers ALL `want` with at least one fallback reason.
fn assert_all(selection: &RootSelection, want: &[&str]) {
    let want: BTreeSet<String> = want.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(selection.selected, want);
    assert!(!selection.fallback.is_empty(), "fallback recorded");
}

// (1) Provider aliases: `provider` blocks are known but untracked.

#[test]
fn provider_default_plus_alias_analyzes() -> Outcome {
    let text = "provider \"aws\" {\n region = \"us-east-1\"\n}\n\
        provider \"aws\" {\n alias = \"west\"\n region = \"us-west-2\"\n}\n";
    let pairs = vec![("stack/main.tf".to_owned(), text.to_owned())];
    let analyzed = analyze_files(&pairs).map_err(|err| err.to_string())?;
    assert_eq!(analyzed.effective, vec!["stack/main.tf".to_owned()]);
    Ok(())
}

#[test]
fn provider_repeats_across_files_not_duplicates() -> Outcome {
    let one = "provider \"aws\" {\n region = \"us-east-1\"\n}\n";
    let two = "provider \"aws\" {\n alias = \"west\"\n}\n";
    let pairs = vec![
        ("stack/one.tf".to_owned(), one.to_owned()),
        ("stack/two.tf".to_owned(), two.to_owned()),
    ];
    analyze_files(&pairs).map_err(|err| err.to_string())?;
    Ok(())
}

#[test]
fn tracked_repeats_still_duplicate_beside_providers() {
    let one = "variable \"name\" {\n type = string\n}\n";
    let two = "variable \"name\" {\n type = string\n}\n";
    let pairs = vec![
        ("stack/one.tf".to_owned(), one.to_owned()),
        ("stack/two.tf".to_owned(), two.to_owned()),
    ];
    let err = analyze_files(&pairs).expect_err("tracked repeat duplicates");
    assert!(err.to_string().contains("duplicate"), "{err}");
}

// (2) Named independent roots: narrow selection, disjoint fmt scopes.

#[test]
fn independent_roots_select_only_the_named_owner() {
    let edges = empty();
    let selection = select_roots(
        &roots(&["stacks/vpc", "stacks/app"]),
        &roots(&["stacks/vpc", "stacks/app"]),
        &edges,
        &edges,
        &changed(&["stacks/app/main.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["stacks/app"]);
}

#[test]
fn independent_roots_hold_disjoint_fmt_scopes() {
    let paths = roots(&[
        "stacks/vpc/main.tf",
        "stacks/vpc/vars.tfvars",
        "stacks/app/main.tf",
    ]);
    let vpc = fmt_scope_for_root(&paths, "stacks/vpc");
    let app = fmt_scope_for_root(&paths, "stacks/app");
    assert_eq!(vpc.len(), 2);
    assert_eq!(app, vec!["stacks/app/main.tf".to_owned()]);
    assert!(vpc.iter().all(|path| !app.contains(path)));
}

// (3) Formatting-only change: selection is path-based, never content-diffed.

#[test]
fn formatting_only_change_still_selects_caller() {
    // select_roots sees changed PATHS only; a whitespace-only edit to
    // a module file still selects the calling root (conservative).
    let mut head = empty();
    head.edges.push(velnor_actions_tofu_core::ModuleEdge {
        from: "a".to_owned(),
        to: "m".to_owned(),
        source: "./../m".to_owned(),
    });
    let base = empty();
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &base,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn non_config_change_under_root_still_selects_owner() {
    // Selection attributes by nearest enclosing node, not by family:
    // even a non-config file under a root selects that root.
    let edges = empty();
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &edges,
        &edges,
        &changed(&["a/notes.txt"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

// (4) External-template impact: real texts widen selection with reasons.

#[test]
fn external_and_template_sources_widen_to_all() -> Outcome {
    let external = "module \"ext\" {\n source = \"/opt/copy\"\n}\n";
    let template = "module \"dyn\" {\n source = \"./${var.env}\"\n}\n";
    let pairs = vec![
        ("a/ext.tf".to_owned(), external.to_owned()),
        ("a/dyn.tf".to_owned(), template.to_owned()),
    ];
    let refs = module_refs_for_texts(&pairs).map_err(|err| err.to_string())?;
    assert_eq!(refs.len(), 2);
    let head = resolve_refs(&refs).map_err(|err| err.to_string())?;
    assert!(head.edges.is_empty(), "no local edges");
    assert!(
        head.findings
            .iter()
            .any(|finding| finding.class == SourceClass::External),
        "{:?}",
        head.findings
    );
    assert!(
        head.findings
            .iter()
            .any(|finding| finding.class == SourceClass::Dynamic),
        "{:?}",
        head.findings
    );
    let base = empty();
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &base,
        &head,
        &changed(&["a/ext.tf"]),
    )
    .map_err(|err| err.to_string())?;
    assert_all(&selection, &["a", "b"]);
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::External { .. })),
        "{:?}",
        selection.fallback
    );
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::Dynamic { .. })),
        "{:?}",
        selection.fallback
    );
    Ok(())
}

// (5) Missing comparison: no base graph still selects from head.

#[test]
fn missing_base_comparison_selects_from_head() {
    // An absent base revision is the empty graph: head topology
    // alone drives narrow selection (no fallback invented).
    let missing_base = empty();
    let mut head = empty();
    head.edges.push(velnor_actions_tofu_core::ModuleEdge {
        from: "a".to_owned(),
        to: "m".to_owned(),
        source: "./../m".to_owned(),
    });
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &missing_base,
        &head,
        &changed(&["m/x.tf"]),
    )
    .expect("selects");
    assert_narrow(&selection, &["a"]);
}

#[test]
fn missing_base_with_empty_head_selects_nothing_for_roots() {
    // No graph anywhere and a change owned by no node widens to ALL
    // with an Unknown reason (fail-closed, never silent nothing).
    let missing_base = empty();
    let head = empty();
    let selection = select_roots(
        &roots(&["a", "b"]),
        &roots(&["a", "b"]),
        &missing_base,
        &head,
        &changed(&["stray/x.tf"]),
    )
    .expect("selects");
    assert_all(&selection, &["a", "b"]);
    assert!(
        selection
            .fallback
            .iter()
            .any(|reason| matches!(reason, SelectAllReason::Unknown { .. })),
        "{:?}",
        selection.fallback
    );
}

// (6) Tool/config reselect: a new tool pin or root set re-decides.

#[test]
fn tool_change_flips_evidence_level() {
    use velnor_actions_tofu_core::EvidenceLevel;
    let files = roots(&["main.tf"]);
    let mut opentofu = BTreeMap::new();
    opentofu.insert("tools.opentofu".to_owned(), "1.13.1".to_owned());
    let before = classify(&files, &opentofu);
    assert_eq!(before.level, EvidenceLevel::Strong);
    let mut mixed = opentofu.clone();
    mixed.insert("tools.terraform".to_owned(), "1.9.0".to_owned());
    let after = classify(&files, &mixed);
    assert_eq!(after.level, EvidenceLevel::Conflict);
}

#[test]
fn config_roots_change_reselects_universe() -> Outcome {
    let dir = TempDir::create("tofu-t27-reselect")?;
    dir.write("a/main.tf", "")?;
    dir.write("b/main.tf", "")?;
    let index = build_index(dir.path(), &[])?;
    let before = qualify_roots("config.toml", dir.path(), &tofu(&["a", "b"]), &index)?;
    assert_eq!(before.len(), 2);
    let after = qualify_roots("config.toml", dir.path(), &tofu(&["a"]), &index)?;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].unit_root, "a");
    // The same change now selects within the narrowed universe.
    let edges = empty();
    let selection = select_roots(
        &roots(&["a"]),
        &roots(&["a", "b"]),
        &edges,
        &edges,
        &changed(&["a/main.tf"]),
    )
    .map_err(|err| err.to_string())?;
    assert_narrow(&selection, &["a"]);
    Ok(())
}
