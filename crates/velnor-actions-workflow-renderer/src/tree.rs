// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_POINTER_BODY, ClaudeMdMode,
};

use crate::agents_md;
use crate::render::{JACTIONLINT_PATH, WORKFLOW_PATH};
use crate::{RenderError, guard, marker, steps};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub path: String,
    pub bytes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSymlink {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTree {
    pub files: Vec<RenderedFile>,
    pub symlinks: Vec<RenderedSymlink>,
}

impl RenderedTree {
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_str())
    }

    #[must_use]
    pub fn get_symlink(&self, path: &str) -> Option<&str> {
        self.symlinks
            .iter()
            .find(|link| link.path == path)
            .map(|link| link.target.as_str())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len() + self.symlinks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.symlinks.is_empty()
    }

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

pub fn render_tree(
    workflow_bytes: &str,
    lint_config_bytes: &str,
    version: &str,
) -> Result<RenderedTree, RenderError> {
    render_tree_with_extra(workflow_bytes, lint_config_bytes, &[], version)
}

pub fn render_tree_with_extra(
    workflow_bytes: &str,
    lint_config_bytes: &str,
    extra: &[RenderedFile],
    version: &str,
) -> Result<RenderedTree, RenderError> {
    render_tree_with_claude_mode(
        workflow_bytes,
        lint_config_bytes,
        extra,
        version,
        ClaudeMdMode::Import,
    )
}

pub fn render_tree_with_claude_mode(
    workflow_bytes: &str,
    lint_config_bytes: &str,
    extra: &[RenderedFile],
    version: &str,
    claude_md: ClaudeMdMode,
) -> Result<RenderedTree, RenderError> {
    marker::check_first_line(workflow_bytes, version)?;
    marker::check_first_line(lint_config_bytes, version)?;
    steps::scan_for_private_subcommands(workflow_bytes)?;
    steps::scan_for_private_subcommands(lint_config_bytes)?;
    guard::validate_tree_path(JACTIONLINT_PATH)?;
    guard::validate_tree_path(WORKFLOW_PATH)?;
    guard::validate_tree_path(AGENTS_MD_PATH)?;
    guard::validate_tree_path(CLAUDE_MD_PATH)?;

    let mut files = vec![
        RenderedFile {
            path: JACTIONLINT_PATH.to_owned(),
            bytes: lint_config_bytes.to_owned(),
        },
        RenderedFile {
            path: WORKFLOW_PATH.to_owned(),
            bytes: workflow_bytes.to_owned(),
        },
        agents_md::render_agents_md(version)?,
    ];
    let symlinks = match claude_md {
        ClaudeMdMode::Import => {
            files.push(RenderedFile {
                path: CLAUDE_MD_PATH.to_owned(),
                bytes: CLAUDE_MD_POINTER_BODY.to_owned(),
            });
            Vec::new()
        }
        ClaudeMdMode::Symlink => vec![RenderedSymlink {
            path: CLAUDE_MD_PATH.to_owned(),
            target: "AGENTS.md".to_owned(),
        }],
    };
    for file in extra {
        marker::check_first_line(&file.bytes, version)?;
        steps::scan_for_private_subcommands(&file.bytes)?;
        guard::validate_tree_path(&file.path)?;
        if file.path == JACTIONLINT_PATH
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
    for file in &files {
        crate::workflow_size::check_workflow_size(&file.path, &file.bytes)?;
    }
    for pair in files.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(RenderError::UnsafePath("tree_path_duplicate".to_owned()));
        }
    }

    Ok(RenderedTree { files, symlinks })
}
