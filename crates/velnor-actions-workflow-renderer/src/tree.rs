//! Generated-tree assembly: marker-checked files in sorted path order.
//!
//! [`render_tree`] builds the exact base files (`actionlint.yaml`, `ci.yml`,
//! `AGENTS.md`, and `CLAUDE.md -> AGENTS.md`); [`render_tree_with_extra`]
//! adds the validated release family. Rendering the workflow bytes stays in
//! [`crate::render`]; this module assembles and validates the generated tree.

use velnor_actions_contract::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET, PULL_REQUEST_TEMPLATE_PATH,
};

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
/// A repository-owned PR template rides separately via
/// [`render_tree_with_preserved`]: preserved bytes skip the marker and
/// token gates but never the path gates.
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
    render_tree_full(workflow_bytes, actionlint_bytes, extra, None, version)
}

/// Assemble the generated tree plus one preserved repository-owned file.
///
/// `preserved` carries the existing [`PULL_REQUEST_TEMPLATE_PATH`] bytes
/// (or `None` when the repository has no template): the path is
/// validated and collision-checked like every extra, but the bytes skip
/// the generated-marker and private-subcommand gates — GitHub renders
/// the template and it never executes. Files are sorted by path, so
/// `None` renders exactly [`render_tree_with_extra`].
///
/// # Errors
///
/// Returns [`RenderError`] for marker, token, path, or collision failures.
pub fn render_tree_with_preserved(
    workflow_bytes: &str,
    actionlint_bytes: &str,
    extra: &[RenderedFile],
    preserved: Option<&RenderedFile>,
    version: &str,
) -> Result<RenderedTree, RenderError> {
    render_tree_full(workflow_bytes, actionlint_bytes, extra, preserved, version)
}

fn render_tree_full(
    workflow_bytes: &str,
    actionlint_bytes: &str,
    extra: &[RenderedFile],
    preserved: Option<&RenderedFile>,
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
        check_base_collision(&file.path)?;
        files.push(file.clone());
    }
    if let Some(template) = preserved {
        if template.path != PULL_REQUEST_TEMPLATE_PATH {
            return Err(RenderError::UnsafePath(format!(
                "preserved_path_rejected:{}",
                template.path
            )));
        }
        guard::validate_tree_path(&template.path)?;
        check_base_collision(&template.path)?;
        files.push(template.clone());
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

/// Reject extras colliding with the four fixed base paths.
fn check_base_collision(path: &str) -> Result<(), RenderError> {
    if path == ACTIONLINT_PATH
        || path == WORKFLOW_PATH
        || path == AGENTS_MD_PATH
        || path == CLAUDE_MD_PATH
    {
        return Err(RenderError::UnsafePath(format!(
            "tree_path_collision:{path}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Marked file bytes for the base inputs.
    fn marked(body: &str) -> String {
        marker::with_marker("0.1.0", body).expect("marked")
    }

    #[test]
    fn preserved_none_matches_extra() {
        let workflow = marked("workflow: true\n");
        let actionlint = marked("actionlint: true\n");
        let plain = render_tree_with_extra(&workflow, &actionlint, &[], "0.1.0").expect("plain");
        let none =
            render_tree_with_preserved(&workflow, &actionlint, &[], None, "0.1.0").expect("none");
        assert_eq!(plain, none);
    }

    #[test]
    fn preserved_template_rides_unmarked_and_unscanned() {
        let workflow = marked("workflow: true\n");
        let actionlint = marked("actionlint: true\n");
        // No generated marker; mentions the runbook without tripping gates.
        let template = RenderedFile {
            path: PULL_REQUEST_TEMPLATE_PATH.to_owned(),
            bytes: "## Checklist\n\nSee the velnor-actions runbook.\n".to_owned(),
        };
        let tree =
            render_tree_with_preserved(&workflow, &actionlint, &[], Some(&template), "0.1.0")
                .expect("preserved");
        assert_eq!(
            tree.get(PULL_REQUEST_TEMPLATE_PATH),
            Some("## Checklist\n\nSee the velnor-actions runbook.\n")
        );
        let mut sorted = tree.paths();
        sorted.sort();
        assert_eq!(tree.paths(), sorted, "paths stay sorted");
    }

    #[test]
    fn preserved_rejects_foreign_paths_and_collisions() {
        let workflow = marked("workflow: true\n");
        let actionlint = marked("actionlint: true\n");
        let foreign = RenderedFile {
            path: ".github/CODEOWNERS".to_owned(),
            bytes: "owned\n".to_owned(),
        };
        let err = render_tree_with_preserved(&workflow, &actionlint, &[], Some(&foreign), "0.1.0")
            .expect_err("foreign path rejected");
        assert!(err.to_string().contains("preserved_path_rejected"), "{err}");
        let extra = RenderedFile {
            path: PULL_REQUEST_TEMPLATE_PATH.to_owned(),
            bytes: marked("extra\n"),
        };
        let template = RenderedFile {
            path: PULL_REQUEST_TEMPLATE_PATH.to_owned(),
            bytes: "template\n".to_owned(),
        };
        let err =
            render_tree_with_preserved(&workflow, &actionlint, &[extra], Some(&template), "0.1.0")
                .expect_err("collision duplicates");
        assert!(err.to_string().contains("tree_path_duplicate"), "{err}");
    }
}
