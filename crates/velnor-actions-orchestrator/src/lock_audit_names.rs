use std::collections::BTreeSet;

use velnor_actions_mise::toolfiles::lockfile::InstallSubject;

pub(super) fn subject_names(subjects: &[InstallSubject]) -> String {
    let names: BTreeSet<&str> = subjects
        .iter()
        .map(|subject| subject.display.as_str())
        .collect();
    names.into_iter().collect::<Vec<_>>().join(", ")
}
