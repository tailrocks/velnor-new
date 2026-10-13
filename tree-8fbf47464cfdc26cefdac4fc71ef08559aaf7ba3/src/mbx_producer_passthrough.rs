//! Copy the two MBX producer files into the generated tree.
//!
//! Java Plan diffs a fresh generate against `.github`. These paths are not
//! schema-2 workflows. Both regular files pass through together. A missing
//! pair adds nothing. Any other path stays out.

use std::path::Path;

use velnor_actions_workflow_renderer::guard;
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree};
use velnor_actions_workflow_renderer::steps::scan_for_private_subcommands;

use crate::OrchestratorError;

const WORKFLOW: &str = ".github/workflows/mbx-single-bundle-producer.yml";
const SCRIPT: &str = ".github/scripts/save-mbx-single-bundle.sh";
const ALLOWED: &[&str] = &[WORKFLOW, SCRIPT];
const MAX_BYTES: u64 = 256 * 1024;

/// Insert the allowlisted producer files when both are regular files.
///
/// # Errors
///
/// Returns [`OrchestratorError::UnsafePath`] for a symlink, a partial pair,
/// a non-file, an oversized payload, or a duplicate path. Returns
/// [`OrchestratorError::Render`] when the bytes contain a private subcommand.
/// Returns [`OrchestratorError::Io`] when the read fails.
pub(crate) fn append_allowlisted(
    root: &Path,
    tree: &mut RenderedTree,
) -> Result<(), OrchestratorError> {
    let workflow = read_member(root, WORKFLOW)?;
    let script = read_member(root, SCRIPT)?;
    match (workflow, script) {
        (None, None) => Ok(()),
        (Some(workflow), Some(script)) => {
            push_file(tree, WORKFLOW, workflow)?;
            push_file(tree, SCRIPT, script)?;
            tree.files.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(())
        }
        (Some(_), None) => Err(partial(SCRIPT)),
        (None, Some(_)) => Err(partial(WORKFLOW)),
    }
}

fn partial(path: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.to_owned(),
        reason: "mbx_producer_partial".to_owned(),
    }
}

fn read_member(root: &Path, rel: &str) -> Result<Option<String>, OrchestratorError> {
    guard::validate_allowlisted_path(rel, ALLOWED)?;
    let path = root.join(rel);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(OrchestratorError::io(
                path.display().to_string(),
                err.to_string(),
            ));
        }
    };
    if meta.is_symlink() {
        return Err(refused(rel, "symlink_refused"));
    }
    if !meta.is_file() {
        return Err(refused(rel, "not_a_file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(refused(rel, "too_large"));
    }
    reject_parent_symlink(root, &path)?;
    let bytes = std::fs::read(&path)
        .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
    let text = String::from_utf8(bytes).map_err(|_| refused(rel, "utf8"))?;
    scan_for_private_subcommands(&text).map_err(|err| OrchestratorError::Render {
        problem: err.to_string(),
    })?;
    Ok(Some(text))
}

fn push_file(tree: &mut RenderedTree, path: &str, bytes: String) -> Result<(), OrchestratorError> {
    if tree.files.iter().any(|file| file.path == path) {
        return Err(refused(path, "tree_path_duplicate"));
    }
    tree.files.push(RenderedFile {
        path: path.to_owned(),
        bytes,
    });
    Ok(())
}

fn refused(path: &str, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}

fn reject_parent_symlink(root: &Path, file: &Path) -> Result<(), OrchestratorError> {
    let mut current = file.parent();
    while let Some(level) = current {
        if level == root {
            break;
        }
        if !level.starts_with(root) {
            return Err(refused(&level.display().to_string(), "escapes_root"));
        }
        if std::fs::symlink_metadata(level).is_ok_and(|meta| meta.is_symlink()) {
            return Err(refused(&level.display().to_string(), "symlink_refused"));
        }
        current = level.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("velnor-mbx-pass-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn empty_tree() -> RenderedTree {
        RenderedTree {
            files: Vec::new(),
            symlinks: Vec::new(),
        }
    }

    fn write_pair(root: &Path, workflow: &str, script: &str) {
        let workflow_path = root.join(WORKFLOW);
        let script_path = root.join(SCRIPT);
        std::fs::create_dir_all(workflow_path.parent().expect("workflow parent")).expect("dir");
        std::fs::create_dir_all(script_path.parent().expect("script parent")).expect("dir");
        std::fs::write(workflow_path, workflow).expect("workflow");
        std::fs::write(script_path, script).expect("script");
    }

    #[test]
    fn absent_pair_leaves_tree_unchanged() {
        let root = scratch("absent");
        let mut tree = empty_tree();
        append_allowlisted(&root, &mut tree).expect("absent");
        assert!(tree.files.is_empty());
    }

    #[test]
    fn both_files_copy_exact_bytes() {
        let root = scratch("both");
        let workflow = "name: MBX bundle producer\n";
        let script = "#!/usr/bin/env bash\nset -euo pipefail\n";
        write_pair(&root, workflow, script);
        std::fs::write(root.join(".github/workflows/other.yml"), "name: other\n").expect("other");
        let mut tree = empty_tree();
        append_allowlisted(&root, &mut tree).expect("copy");
        assert_eq!(tree.get(WORKFLOW), Some(workflow));
        assert_eq!(tree.get(SCRIPT), Some(script));
        assert_eq!(tree.get(".github/workflows/other.yml"), None);
        assert_eq!(tree.files.len(), 2);
    }

    #[test]
    fn one_file_is_partial() {
        let root = scratch("partial");
        let workflow_path = root.join(WORKFLOW);
        std::fs::create_dir_all(workflow_path.parent().expect("parent")).expect("dir");
        std::fs::write(&workflow_path, "name: only\n").expect("one");
        let mut tree = empty_tree();
        let err = append_allowlisted(&root, &mut tree).expect_err("partial");
        assert!(err.to_string().contains("mbx_producer_partial"), "{err}");
        assert!(tree.files.is_empty());
    }

    #[test]
    fn symlink_member_is_refused() {
        let root = scratch("symlink");
        let outside = scratch("symlink-target");
        std::fs::write(outside.join("real.sh"), "#!/usr/bin/env bash\n").expect("real");
        let workflow_path = root.join(WORKFLOW);
        let script_path = root.join(SCRIPT);
        std::fs::create_dir_all(workflow_path.parent().expect("parent")).expect("dir");
        std::fs::create_dir_all(script_path.parent().expect("script parent")).expect("dir");
        std::fs::write(&workflow_path, "name: MBX\n").expect("workflow");
        symlink(outside.join("real.sh"), &script_path).expect("link");
        let mut tree = empty_tree();
        let err = append_allowlisted(&root, &mut tree).expect_err("symlink");
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        assert!(tree.files.is_empty());
    }

    #[test]
    fn parent_symlink_is_refused() {
        let root = scratch("parent-link");
        let real = scratch("parent-real");
        write_pair(&real, "name: MBX\n", "#!/usr/bin/env bash\n");
        symlink(real.join(".github"), root.join(".github")).expect("dir link");
        let mut tree = empty_tree();
        let err = append_allowlisted(&root, &mut tree).expect_err("parent symlink");
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        assert!(tree.files.is_empty());
    }

    #[test]
    fn forbidden_token_is_refused() {
        let root = scratch("token");
        write_pair(
            &root,
            "name: MBX\n",
            "#!/usr/bin/env bash\nvelnor-actions run\n",
        );
        let mut tree = empty_tree();
        let err = append_allowlisted(&root, &mut tree).expect_err("token");
        assert!(err.to_string().contains("velnor-actions run"), "{err}");
        assert!(tree.files.is_empty());
    }
}
