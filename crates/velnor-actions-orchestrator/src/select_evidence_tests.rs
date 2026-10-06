//! Qualified directory hints still require independent semantic proof.

use std::collections::BTreeSet;

use super::ChangedSelection;

#[path = "select_evidence_git_tests.rs"]
mod git_tests;

fn units(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn qualified_ownership_hints_preserve_full_affected_set() {
    let all = units(&["source", "dependent", "unrelated"]);
    let selection = ChangedSelection::qualified(all.clone());
    assert_eq!(selection.affected, all);
    assert_eq!(selection.proof_refinable, all);
}

#[test]
fn unknown_comparison_and_config_broadening_have_no_refinement() {
    let selection = ChangedSelection::required(units(&["source", "dependent"]));
    assert!(selection.proof_refinable.is_empty());
    assert_eq!(selection.affected, units(&["source", "dependent"]));
}

#[test]
fn unowned_only_changes_keep_full_universe_until_verified_coverage() {
    let all = units(&["one", "two"]);
    let selection = ChangedSelection::qualified(all.clone());
    assert_eq!(selection.affected, all);
    assert_eq!(selection.proof_refinable, all);
}
