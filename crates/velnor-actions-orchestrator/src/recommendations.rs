//! Sorted unique recommendation collection for plan and generate.
//!
//! Merges profile recommendations, tool-file presence notes, adapter
//! check lines, and value-conflict findings into the shared text both
//! commands report.

use std::collections::BTreeSet;

use velnor_actions_rust::FileIndex;

use crate::discover::PlannedWorkspace;
use crate::toolcheck::ToolInputCheck;
use crate::toolfindings::{finding_line, tool_check_lines, tool_conflicts};

/// Collect profile plus tool-file recommendations, sorted and unique.
pub(crate) fn collect_recommendations(
    index: &FileIndex,
    workspaces: &[PlannedWorkspace],
    tool_checks: &[ToolInputCheck],
) -> Vec<String> {
    let mut out = BTreeSet::new();
    for workspace in workspaces {
        for recommendation in &workspace.recommendations {
            out.insert(format!(
                "{}: {}",
                recommendation.code, recommendation.message
            ));
        }
    }
    for line in tool_check_lines(tool_checks) {
        out.insert(line);
    }
    for finding in tool_conflicts(tool_checks) {
        out.insert(finding_line(&finding));
    }
    if index.contains("mise.toml") || index.contains(".mise.toml") {
        out.insert("mise.toml is read-only input; Velnor never modifies it".to_owned());
    } else {
        out.insert("mise.toml not found; Velnor will use its pinned tools".to_owned());
    }
    if index.contains("mise.lock") {
        out.insert("mise.lock is read-only input; Velnor never modifies it".to_owned());
    } else {
        out.insert("mise.lock not found; Velnor will not create or refresh it".to_owned());
    }
    if index.contains("rust-toolchain.toml") {
        out.insert("rust-toolchain.toml is read-only input; Velnor never modifies it".to_owned());
    } else {
        out.insert("rust-toolchain.toml not found; Velnor will not create it".to_owned());
    }
    if index.contains(".github/CODEOWNERS") {
        out.insert(
            "CODEOWNERS inside .github is removed on generate; keep it at the repository root or under docs/"
                .to_owned(),
        );
    }
    out.into_iter().collect()
}
