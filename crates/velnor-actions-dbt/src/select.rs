//! Affected-project claims (hard decision 3 for the dbt stack).
//!
//! A project job is selected when a changed path lies in the project
//! directory. Reads outside the directory are refused by `analyze_project`
//! (`dbt_path_escapes_project`), so the directory is the whole input set; a
//! repository-root project (`.`) is selected by any change.

use std::collections::BTreeSet;

use crate::paths::is_under;

/// Directories (from `projects`) affected by `changed` repository-relative paths.
#[must_use]
pub fn affected_projects(projects: &[String], changed: &BTreeSet<String>) -> BTreeSet<String> {
    projects
        .iter()
        .filter(|dir| changed.iter().any(|path| is_under(path, dir)))
        .cloned()
        .collect()
}
