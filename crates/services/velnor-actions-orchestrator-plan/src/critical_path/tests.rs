use super::*;

/// Weights plus predecessor edges for tests.
fn graph(weights: &[(&str, u64)], edges: &[(&str, &[&str])]) -> CriticalPath {
    let durations: BTreeMap<String, u64> = weights
        .iter()
        .map(|(id, ms)| ((*id).to_owned(), *ms))
        .collect();
    let predecessors: BTreeMap<String, Vec<String>> = edges
        .iter()
        .map(|(id, preds)| {
            (
                (*id).to_owned(),
                preds.iter().map(|pred| (*pred).to_owned()).collect(),
            )
        })
        .collect();
    critical_path(&durations, &predecessors)
}

#[test]
fn longest_weighted_chain_wins() {
    let path = graph(
        &[("a", 5), ("b", 50), ("c", 6), ("d", 7)],
        &[("c", &["a"]), ("d", &["b"])],
    );
    assert_eq!(path.path, vec!["b".to_owned(), "d".to_owned()]);
    assert_eq!(path.total_duration_ms, 57);
}

#[test]
fn unknown_predecessors_are_zero_roots_and_cycles_terminate() {
    let path = graph(&[("a", 3)], &[("a", &["external"])]);
    assert_eq!(path.path, vec!["a".to_owned()]);
    assert_eq!(path.total_duration_ms, 3);
    let cyclic = graph(&[("a", 2), ("b", 4)], &[("a", &["b"]), ("b", &["a"])]);
    assert_eq!(cyclic.total_duration_ms, 6);
    assert_eq!(cyclic.path.len(), 2);
}

#[test]
fn ties_resolve_to_smallest_chain_head() {
    let path = graph(&[("b", 1), ("a", 1)], &[]);
    assert_eq!(path.path, vec!["a".to_owned()]);
}

#[test]
fn structural_path_weighs_one_per_task() {
    let path = graph(
        &[("a", 1), ("b", 1), ("c", 1)],
        &[("b", &["a"]), ("c", &["b"])],
    );
    assert_eq!(path.path.len(), 3);
    assert_eq!(path.total_duration_ms, 3);
}

#[test]
fn render_lists_chain_total_and_task_durations() {
    let path = graph(&[("a", 5), ("c", 6)], &[("c", &["a"])]);
    let durations: BTreeMap<String, u64> = [("a".to_owned(), 5), ("c".to_owned(), 6)]
        .into_iter()
        .collect();
    let lines = render_critical_path(&path, &durations);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("total 11 ms"), "{lines:?}");
    assert!(lines[1].contains("a: 5 ms"), "{lines:?}");
    assert!(lines[2].contains("c: 6 ms"), "{lines:?}");
    assert!(critical_path_line(&path).contains("a -> c"));
    let empty = CriticalPath {
        path: Vec::new(),
        total_duration_ms: 0,
    };
    assert!(critical_path_line(&empty).contains("none"));
}
