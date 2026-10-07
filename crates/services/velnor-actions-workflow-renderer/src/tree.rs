//! Generated-tree assembly: marker-checked files in sorted path order.
//!
//! [`render_tree`] builds the exact base files (`actionlint.yaml`, `ci.yml`,
//! `AGENTS.md`, and `CLAUDE.md -> AGENTS.md`); [`render_tree_with_extra`]
//! adds the validated release family. Rendering the workflow bytes stays in
//! [`crate::render`]; this module assembles and validates the generated tree.

use velnor_actions_contract_release::{AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET};
use velnor_actions_contract_workflow::CI_WORKFLOW_PATH;
use velnor_actions_workflow_steps::{RenderError, steps};
use velnor_actions_workflow_tree::{
    agents_md, guard, marker,
    rendered::{ACTIONLINT_PATH, RenderedFile, RenderedSymlink, RenderedTree},
    workflow_size,
};

/// Assemble the exact base tree from rendered workflow bytes plus the
/// actionlint crate's bytes (passed through, marker-checked).
///
/// # Errors
///
/// Returns [`RenderError`] for marker, token, or path failures.
pub fn render_tree(
    workflow_bytes: &str,
    actionlint_bytes: &str,
    version: &str,
) -> Result<RenderedTree, RenderError> {
    render_tree_with_extra(workflow_bytes, actionlint_bytes, &[], version)
}

/// Assemble the generated tree: base files plus validated extras.
///
/// Extras (the release family) pass the same marker, token, and path
/// gates; base-path collisions and duplicate paths fail closed. Files are
/// sorted by path, so an empty `extra` renders exactly [`render_tree`].
///
/// # Errors
///
/// Returns [`RenderError`] for marker, token, path, or collision failures.
pub fn render_tree_with_extra(
    workflow_bytes: &str,
    actionlint_bytes: &str,
    extra: &[RenderedFile],
    version: &str,
) -> Result<RenderedTree, RenderError> {
    marker::check_first_line(workflow_bytes, version)?;
    marker::check_first_line(actionlint_bytes, version)?;
    steps::scan_for_private_subcommands(workflow_bytes)?;
    steps::scan_for_private_subcommands(actionlint_bytes)?;
    guard::validate_tree_path(ACTIONLINT_PATH)?;
    guard::validate_tree_path(CI_WORKFLOW_PATH)?;
    guard::validate_tree_path(AGENTS_MD_PATH)?;
    guard::validate_tree_path(CLAUDE_MD_PATH)?;

    let agents_file = agents_md::render_agents_md(version)?;

    let mut files = vec![
        RenderedFile {
            path: ACTIONLINT_PATH.to_owned(),
            bytes: actionlint_bytes.to_owned(),
        },
        RenderedFile {
            path: CI_WORKFLOW_PATH.to_owned(),
            bytes: workflow_bytes.to_owned(),
        },
        agents_file,
    ];
    for file in extra {
        marker::check_first_line(&file.bytes, version)?;
        steps::scan_for_private_subcommands(&file.bytes)?;
        guard::validate_tree_path(&file.path)?;
        if file.path == ACTIONLINT_PATH
            || file.path == CI_WORKFLOW_PATH
            || file.path == AGENTS_MD_PATH
            || file.path == CLAUDE_MD_PATH
        {
            return Err(RenderError::UnsafePath(format!(
                "tree_path_collision:{}",
                file.path
            )));
        }
        files.push(file.clone());
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    for file in &files {
        workflow_size::check_workflow_size(&file.path, &file.bytes)?;
    }
    for pair in files.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(RenderError::UnsafePath("tree_path_duplicate".to_owned()));
        }
    }

    let symlinks = vec![RenderedSymlink {
        path: CLAUDE_MD_PATH.to_owned(),
        target: CLAUDE_MD_TARGET.to_owned(),
    }];

    Ok(RenderedTree { files, symlinks })
}
