/// Stage under the root, then commit under the ownership lock.
///
/// Nothing before the commit touches old output; success may carry
/// old-tree-removal warnings.
fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
    _retired: Option<&mut RetiredTreeOwners>,
) -> Result<Vec<String>, OrchestratorError> {
    let root = &prep.root;
    let _ownership = GenerateOwnership::acquire(root)?;
    let target = root.join(".github");
    reject_symlink(&target)?;
    let staging = stage_root::StageRoot::open(root)?;
    let target_identity = staging.validate_target(&target)?;
    staging.validate_target_directories(&target, target_identity)?;
    reject_symlink(&target.join("workflows"))?;
    for rel in check_tree_paths(tree)? {
        check_no_symlink_or_emitted_leaf(root, &rel)?;
    }
    staging.clear_spare_before_staging()?;
    let staged = match preserve::stage_in_place(&target, staging.spare(), tree) {
        Ok(path) => path,
        Err(error) => return Err(staging.abort_before_commit(error)),
    };
    if let Err(error) = staging.validate_staged(&target, target_identity, &staged) {
        return Err(staging.abort_before_commit(error));
    }
    publish_in_place(&staging, &target, target_identity, &staged)
}

/// Publish through the spare slot without removing its root directory.
fn publish_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    target_identity: Option<stage_root::FsIdentity>,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    match target_identity {
        Some(retired) => publish_existing_in_place(staging, target, retired, staged),
        None => publish_first_in_place(staging, target, staged),
    }
}

/// Exchange existing output and remove only children of the retired root.
fn publish_existing_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    retired: stage_root::FsIdentity,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    if let Err(error) = staging.validate_target_directories(target, Some(retired)) {
        return Err(staging.abort_before_commit(error));
    }
    if let Err(error) = exchange_directories(staged, target) {
        return Err(staging.abort_before_commit(OrchestratorError::io(
            target.display().to_string(),
            error.to_string(),
        )));
    }
    match staging.clean_retired_root(retired) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Ok(vec![format!("backup_cleanup_failed:{error}")]),
    }
}

/// Move a first generated tree out of the spare and preserve its root.
fn publish_first_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    match std::fs::rename(staged, target) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Err(staging.abort_before_commit(OrchestratorError::io(
            target.display().to_string(),
            error.to_string(),
        ))),
    }
}

/// Commit fresh output with one rename and existing output with one exchange.
fn swap_directories(
    root: &Path,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    swap_directories_with_cleanup_retention(root, target, staged, false)
}

fn swap_directories_with_cleanup_retention(
    _root: &Path,
    target: &Path,
    staged: &Path,
    retain_cleanup: bool,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    if !target.exists() {
        return match std::fs::rename(staged, target) {
            Ok(()) => Ok(Vec::new()),
            Err(err) => Err(OrchestratorError::io(label, err.to_string())),
        };
    }
    swap_existing_directory_with_cleanup_retention(
        target,
        staged,
        exchange_directories,
        retain_cleanup,
    )
}

/// Exchange an existing target before cleaning up its old tree.
#[cfg(test)]
fn swap_existing_directory(
    target: &Path,
    staged: &Path,
    exchange: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<Vec<String>, OrchestratorError> {
    swap_existing_directory_with_cleanup_retention(target, staged, exchange, false)
}

fn swap_existing_directory_with_cleanup_retention(
    target: &Path,
    staged: &Path,
    exchange: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    _retain_cleanup: bool,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    let exchange_event = atomic_diagnostic_begin_exchange(target, staged);
    let exchanged = exchange(staged, target);
    atomic_diagnostic_finish(
        exchange_event,
        exchanged.is_ok(),
        exchanged
            .as_ref()
            .err()
            .and_then(std::io::Error::raw_os_error),
    );
    exchanged.map_err(|err| OrchestratorError::io(label, err.to_string()))?;
    atomic_diagnostic_hold_retired_root(staged);
    let old = staged.display().to_string();
    if atomic_diagnostic_defer_cleanup() {
        atomic_diagnostic_event(AtomicDiagnosticKind::OldTreeRemovalDeferred);
        return Ok(Vec::new());
    }
    let remove_event = atomic_diagnostic_begin(
        AtomicDiagnosticKind::OldTreeRemoval,
        AtomicDiagnosticIdentity::default(),
        AtomicDiagnosticIdentity::default(),
    );
    let removed = std::fs::remove_dir_all(staged);
    atomic_diagnostic_finish(
        remove_event,
        removed.is_ok(),
        removed
            .as_ref()
            .err()
            .and_then(std::io::Error::raw_os_error),
    );
    match removed {
        Ok(()) => Ok(Vec::new()),
        Err(cleanup) => Ok(vec![format!("backup_cleanup_failed:{old}:{cleanup}")]),
    }
}

/// Use the platform's atomic exchange primitive or fail closed.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exchange_directories(staged: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, staged, CWD, target, RenameFlags::EXCHANGE).map_err(std::io::Error::from)
}

/// Existing output cannot be replaced atomically on unsupported platforms.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exchange_directories(_staged: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic directory exchange is unsupported on this platform",
    ))
}

/// Staged tree writing: regular files and symbolic links.
#[path = "generate_write.rs"]
pub(crate) mod write;

pub(crate) use write::write_tree;

#[cfg(test)]
#[path = "generate_swap_tests.rs"]
mod tests;
