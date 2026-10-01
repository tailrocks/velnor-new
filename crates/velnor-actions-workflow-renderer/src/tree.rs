//! Generated-tree assembly: marker-checked files in sorted path order.
//!
//! [`render_tree`] builds the exact base two files; [`render_tree_with_extra`]
//! adds the validated release family. Rendering the workflow bytes stays in
//! [`crate::render`]; this module only assembles bytes into the tree.

use crate::render::{ACTIONLINT_PATH, RenderedFile, RenderedTree, WORKFLOW_PATH};
use crate::{RenderError, guard, marker, steps};

/// Assemble the exact two-file tree from rendered workflow bytes plus the
/// actionlint crate's bytes (passed through, marker-checked).
///
/// Byte-identical to the pre-release tree: delegates with no extras.
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

/// Assemble the generated tree: base two files plus validated extras.
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
    guard::validate_tree_path(WORKFLOW_PATH)?;
    let mut files = vec![
        RenderedFile {
            path: ACTIONLINT_PATH.to_owned(),
            bytes: actionlint_bytes.to_owned(),
        },
        RenderedFile {
            path: WORKFLOW_PATH.to_owned(),
            bytes: workflow_bytes.to_owned(),
        },
    ];
    for file in extra {
        marker::check_first_line(&file.bytes, version)?;
        steps::scan_for_private_subcommands(&file.bytes)?;
        guard::validate_tree_path(&file.path)?;
        if file.path == ACTIONLINT_PATH || file.path == WORKFLOW_PATH {
            return Err(RenderError::UnsafePath(format!(
                "tree_path_collision:{}",
                file.path
            )));
        }
        files.push(file.clone());
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    for pair in files.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(RenderError::UnsafePath("tree_path_duplicate".to_owned()));
        }
    }
    Ok(RenderedTree { files })
}
