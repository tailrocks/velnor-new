use std::collections::BTreeSet;
use velnor_actions_dbt::affected_projects;

fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn a_change_under_a_project_selects_only_it() {
    let projects = vec!["analytics".to_owned(), "chainargos-dbt".to_owned()];
    let selected = affected_projects(&projects, &changed(&["chainargos-dbt/models/a.sql"]));
    assert_eq!(selected.into_iter().collect::<Vec<_>>(), ["chainargos-dbt"]);
    assert_eq!(
        affected_projects(&projects, &changed(&["chainargos-dbt2/a"])).len(),
        0
    );
    assert_eq!(
        affected_projects(&projects, &changed(&["README.md"])).len(),
        0
    );
}

#[test]
fn a_root_project_is_selected_by_any_change() {
    let projects = vec![".".to_owned()];
    assert_eq!(affected_projects(&projects, &changed(&["x/y"])).len(), 1);
}
