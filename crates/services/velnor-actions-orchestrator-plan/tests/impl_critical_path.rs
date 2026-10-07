//! Longest duration-weighted chain plus its report lines.

use std::collections::BTreeMap;

use velnor_actions_orchestrator_plan::critical_path::{
    CriticalPath, critical_path, critical_path_line, render_critical_path,
};

fn weights(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
    entries
        .iter()
        .map(|(id, weight)| ((*id).to_owned(), *weight))
        .collect()
}

fn edges(entries: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    entries
        .iter()
        .map(|(id, preds)| {
            (
                (*id).to_owned(),
                preds.iter().map(ToString::to_string).collect(),
            )
        })
        .collect()
}

#[test]
fn empty_graph_yields_empty_path() {
    assert_eq!(
        critical_path(&BTreeMap::new(), &BTreeMap::new()),
        CriticalPath {
            path: Vec::new(),
            total_duration_ms: 0,
        }
    );
}

#[test]
fn chain_follows_heaviest_route() {
    let path = critical_path(
        &weights(&[("a", 1), ("b", 5), ("c", 1)]),
        &edges(&[("b", &["a"]), ("c", &["b"])]),
    );
    assert_eq!(path.path, ["a", "b", "c"]);
    assert_eq!(path.total_duration_ms, 7);
}

#[test]
fn ties_resolve_to_smallest_chain_head() {
    let path = critical_path(&weights(&[("a", 1), ("b", 1)]), &BTreeMap::new());
    assert_eq!(path.path, ["a"]);
}

#[test]
fn unknown_predecessors_are_zero_roots() {
    let path = critical_path(&weights(&[("a", 3)]), &edges(&[("a", &["missing"])]));
    assert_eq!(path.path, ["a"]);
    assert_eq!(path.total_duration_ms, 3);
}

#[test]
fn line_formats_chain_and_empty() {
    assert_eq!(
        critical_path_line(&CriticalPath {
            path: vec!["a".to_owned(), "b".to_owned()],
            total_duration_ms: 7,
        }),
        "Critical path: a -> b (2 tasks)"
    );
    assert_eq!(
        critical_path_line(&CriticalPath {
            path: Vec::new(),
            total_duration_ms: 0,
        }),
        "Critical path: none (no obligations)"
    );
}

#[test]
fn render_lists_chain_total_and_task_durations() {
    let lines = render_critical_path(
        &CriticalPath {
            path: vec!["a".to_owned(), "b".to_owned()],
            total_duration_ms: 7,
        },
        &weights(&[("a", 2), ("b", 5)]),
    );
    assert_eq!(
        lines,
        [
            "Critical path: a -> b (2 tasks, total 7 ms)",
            "  a: 2 ms",
            "  b: 5 ms",
        ]
    );
}
