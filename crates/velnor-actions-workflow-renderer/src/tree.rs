//! Generated-tree assembly: marker-checked files in sorted path order.
//!
//! [`render_tree`] builds the exact base files (`actionlint.yaml`, `ci.yml`,
//! `AGENTS.md`, and `CLAUDE.md -> AGENTS.md`); [`render_tree_with_extra`]
//! adds the validated release family. Rendering the workflow bytes stays in
//! [`crate::render`]; this module assembles and validates the generated tree.

use velnor_actions_contract::{AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET};

use crate::agents_md;
use crate::render::{ACTIONLINT_PATH, WORKFLOW_PATH};
use crate::{RenderError, guard, marker, steps};

/// One rendered file: repository-relative path plus bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    /// Repository-relative output path.
    pub path: String,
    /// Complete file bytes including the marker.
    pub bytes: String,
}

/// One rendered symbolic link: repository-relative link path plus target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSymlink {
    /// Repository-relative symlink path.
    pub path: String,
    /// Relative target path.
    pub target: String,
}

/// The generated files and symlinks, sorted by path: the base files
/// (actionlint config, CI workflow, AGENTS.md, and CLAUDE.md symlink) with release
/// disabled, plus the release family when release rendering is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTree {
    /// Generated files in sorted path order.
    pub files: Vec<RenderedFile>,
    /// Generated symbolic links in sorted path order.
    pub symlinks: Vec<RenderedSymlink>,
}

impl RenderedTree {
    /// Fetch file bytes by repository-relative path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_str())
    }

    /// Fetch symlink target by repository-relative path.
    #[must_use]
    pub fn get_symlink(&self, path: &str) -> Option<&str> {
        self.symlinks
            .iter()
            .find(|link| link.path == path)
            .map(|link| link.target.as_str())
    }

    /// Total count of all generated items (files plus symlinks).
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len() + self.symlinks.len()
    }

    /// Whether the tree contains no files and no symlinks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.symlinks.is_empty()
    }

    /// Sorted repository-relative paths of all files and symlinks.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .files
            .iter()
            .map(|f| f.path.clone())
            .chain(self.symlinks.iter().map(|s| s.path.clone()))
            .collect();
        paths.sort();
        paths
    }
}

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
    guard::validate_tree_path(WORKFLOW_PATH)?;
    guard::validate_tree_path(AGENTS_MD_PATH)?;
    guard::validate_tree_path(CLAUDE_MD_PATH)?;

    let agents_file = agents_md::render_agents_md(version)?;

    let mut files = vec![
        RenderedFile {
            path: ACTIONLINT_PATH.to_owned(),
            bytes: actionlint_bytes.to_owned(),
        },
        RenderedFile {
            path: WORKFLOW_PATH.to_owned(),
            bytes: workflow_bytes.to_owned(),
        },
        agents_file,
    ];
    for file in extra {
        marker::check_first_line(&file.bytes, version)?;
        steps::scan_for_private_subcommands(&file.bytes)?;
        guard::validate_tree_path(&file.path)?;
        if file.path == ACTIONLINT_PATH
            || file.path == WORKFLOW_PATH
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
