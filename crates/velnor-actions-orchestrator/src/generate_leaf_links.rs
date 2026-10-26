// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};

use crate::OrchestratorError;

pub(crate) fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_symlink())
}

pub(crate) fn check_no_symlink_or_emitted_leaf(
    root: &Path,
    rel: &SafeTreePath,
) -> Result<(), OrchestratorError> {
    match guard::check_no_symlink(root, rel, is_symlink) {
        Ok(()) => Ok(()),
        Err(err) => {
            if emitted_leaf_symlink_only(root, rel) {
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }
}

fn emitted_leaf_symlink_only(root: &Path, rel: &SafeTreePath) -> bool {
    let mut segments = rel.as_str().split('/');
    let Some(leaf) = segments.next_back() else {
        return false;
    };
    let mut prefix = root.to_path_buf();
    for segment in segments {
        prefix.push(segment);
        if is_symlink(&prefix) {
            return false;
        }
    }
    prefix.push(leaf);
    is_symlink(&prefix)
}
