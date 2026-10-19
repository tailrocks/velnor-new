//! Declaration-level change check for project tool sources (hard decision 5).
//!
//! CI runs the tool versions the project declares, so a change to a version
//! must re-test every crate job. A change to anything else in the same file
//! (a task body, a Node pin, a comment) must not: the declared catalog
//! versions are compared between base and head instead of whole files.
//! Any doubt (a git failure on both sides, a file that does not parse, a
//! lock file) selects everything.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::config::project_tool_refs::{
    is_declaration_source, is_project_tool_lock,
};
use velnor_actions_mise::GitRequest;

use crate::project_tool_sources::declared_versions;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};
use crate::validators::validate_select_show_args;

/// Warning recorded when a project tool declaration (or lock) changed.
pub(super) const PROJECT_TOOL_CONFIG_CHANGED: &str = "project_tool_config_changed:all_changed";

/// Whether `path` can carry a project tool version or pin.
pub(super) fn is_project_tool_source(path: &str) -> bool {
    is_declaration_source(path) || is_project_tool_lock(path)
}

/// Declaration sources in `changed` whose declared catalog versions are the
/// same at base and head: they select nothing. `None` broadens (the
/// warning is recorded).
///
/// `head` is the committed head revision, or `None` to read the working
/// tree (local classification).
pub(super) fn unchanged_declaration_sources(
    root: &Path,
    base: &str,
    head: Option<&str>,
    changed: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    let mut unchanged = BTreeSet::new();
    for path in changed.iter().filter(|path| is_project_tool_source(path)) {
        if !is_declaration_source(path) || !same_declarations(root, base, head, path) {
            warnings.push(PROJECT_TOOL_CONFIG_CHANGED.to_owned());
            return None;
        }
        unchanged.insert(path.clone());
    }
    Some(unchanged)
}

/// True only when both sides parse and declare identical catalog versions.
fn same_declarations(root: &Path, base: &str, head: Option<&str>, path: &str) -> bool {
    let before = blob(root, &format!("{base}:{path}"));
    let after = match head {
        Some(head) => blob(root, &format!("{head}:{path}")),
        None => match read_repo_file(root, path, MAX_REPO_FILE_BYTES) {
            Ok(RepoRead::Text(text)) => Some(text),
            Ok(RepoRead::Absent) => None,
            Err(_) => return false,
        },
    };
    if before.is_none() && after.is_none() {
        return false;
    }
    let declared = |text: &Option<String>| -> Option<_> {
        match text {
            Some(text) => declared_versions(path, text).ok(),
            None => Some(BTreeMap::new()),
        }
    };
    matches!((declared(&before), declared(&after)), (Some(a), Some(b)) if a == b)
}

/// Contents of one `rev:path` blob; a path absent at that revision is `None`.
fn blob(root: &Path, spec: &str) -> Option<String> {
    let args = vec![OsString::from(spec)];
    validate_select_show_args(&args).ok()?;
    let output = GitRequest::show(args).run_in(root).ok()?;
    if !output.success {
        return None;
    }
    output.stdout_text("git").ok()
}

#[cfg(test)]
#[path = "select_tool_sources_tests.rs"]
mod tests;
