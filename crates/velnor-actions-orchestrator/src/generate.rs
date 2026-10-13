//! Staged `.github` replacement and outside-repo preview writes.
//! In-place commits use atomic directory exchange on supported platforms and
//! fail closed when an existing tree cannot be exchanged.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract::{ExecutionMode, expand_workflow};
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::RenderedTree;
use velnor_actions_workflow_renderer::steps::rehead_actionlint_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_extra;

use crate::OrchestratorError;
use crate::finalized::owned_preparation;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::provenance::{ProfileProvenance, profile_provenance};
use crate::validate::validate_staged;
use crate::workflow::scale_set_selector_for_dispatch;

#[derive(Debug, Clone, Copy)]
enum AtomicDiagnosticKind {
    TempDirCreated,
    StageAtTempDirRoot,
    StageNestedGithub,
    StageComplete,
    DirectoryExchange,
    OldTreeRemoval,
    OldTreeRemovalDeferred,
    TempDirDropBegin,
    TempDirDropEnd,
}

#[derive(Debug, Clone, Copy, Default)]
struct AtomicDiagnosticIdentity {
    device: u64,
    inode: u64,
    valid: bool,
    raw_errno: i32,
}

#[derive(Debug, Clone, Copy)]
struct AtomicDiagnosticEvent {
    kind: AtomicDiagnosticKind,
    started_ns: u64,
    ended_ns: u64,
    succeeded: i8,
    raw_errno: i32,
    target_identity: AtomicDiagnosticIdentity,
    staged_identity: AtomicDiagnosticIdentity,
}

#[derive(Debug)]
struct AtomicDiagnosticCapture {
    started: Instant,
    events: Vec<AtomicDiagnosticEvent>,
    defer_cleanup: bool,
    deferred_staging: Vec<tempfile::TempDir>,
    retired_roots: Vec<AtomicDiagnosticRetiredRoot>,
}

#[derive(Debug)]
struct AtomicDiagnosticRetiredRoot {
    file: Option<std::fs::File>,
    before: AtomicDiagnosticIdentity,
    open_errno: i32,
}

thread_local! {
    static ATOMIC_DIAGNOSTIC_EVENTS: RefCell<Option<AtomicDiagnosticCapture>> =
        const { RefCell::new(None) };
}

/// Enable a preallocated trace buffer on the dedicated writer thread.
#[doc(hidden)]
pub fn begin_atomic_diagnostic_capture(started: Instant, defer_cleanup: bool) {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|events| {
        *events.borrow_mut() = Some(AtomicDiagnosticCapture {
            started,
            events: Vec::with_capacity(64),
            defer_cleanup,
            deferred_staging: Vec::with_capacity(8),
            retired_roots: Vec::with_capacity(8),
        });
    });
}

/// Take the writer's preallocated records after its rewrites have finished.
#[doc(hidden)]
pub fn take_atomic_diagnostic_events() -> Option<AtomicDiagnosticTrace> {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|events| events.borrow_mut().take().map(AtomicDiagnosticTrace))
}

/// A writer-thread trace rendered only after the writer and observers join.
#[doc(hidden)]
#[derive(Debug)]
pub struct AtomicDiagnosticTrace(AtomicDiagnosticCapture);

impl AtomicDiagnosticTrace {
    /// Format buffered records outside the generation/observer window.
    #[doc(hidden)]
    pub fn finish_after_observers(self) -> Vec<String> {
        let mut capture = self.0;
        let mut output = capture
            .events
            .into_iter()
            .map(|event| {
                format!(
                    "event={:?} start_ns={} end_ns={} succeeded={} raw_errno={} target_dev={} target_ino={} target_valid={} target_errno={} staged_dev={} staged_ino={} staged_valid={} staged_errno={}",
                    event.kind,
                    event.started_ns,
                    event.ended_ns,
                    event.succeeded,
                    event.raw_errno,
                    event.target_identity.device,
                    event.target_identity.inode,
                    event.target_identity.valid,
                    event.target_identity.raw_errno,
                    event.staged_identity.device,
                    event.staged_identity.inode,
                    event.staged_identity.valid,
                    event.staged_identity.raw_errno
                )
            })
            .collect::<Vec<_>>();
        for (index, staging) in capture.deferred_staging.iter().enumerate() {
            output.push(format!(
                "DEFERRED_CLEANUP index={index} tempdir={} phase=before_drop_after_observers",
                staging.path().display()
            ));
        }
        drop(std::mem::take(&mut capture.deferred_staging));
        for (index, retired) in capture.retired_roots.iter().enumerate() {
            let after = match retired.file.as_ref().map(std::fs::File::metadata) {
                Some(Ok(metadata)) => metadata_identity(metadata),
                Some(Err(error)) => AtomicDiagnosticIdentity {
                    raw_errno: error.raw_os_error().unwrap_or(-1),
                    ..AtomicDiagnosticIdentity::default()
                },
                None => AtomicDiagnosticIdentity {
                    raw_errno: retired.open_errno,
                    ..AtomicDiagnosticIdentity::default()
                },
            };
            output.push(format!(
                "RETIRED_ROOT_FD index={index} open_errno={} pre_dev={} pre_ino={} pre_valid={} post_dev={} post_ino={} post_valid={} post_errno={}",
                retired.open_errno,
                retired.before.device,
                retired.before.inode,
                retired.before.valid,
                after.device,
                after.inode,
                after.valid,
                after.raw_errno
            ));
        }
        output
    }
}

fn metadata_identity(metadata: std::fs::Metadata) -> AtomicDiagnosticIdentity {
    use std::os::unix::fs::MetadataExt;
    AtomicDiagnosticIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        valid: true,
        raw_errno: 0,
    }
}

fn atomic_diagnostic_defer_cleanup() -> bool {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|capture| capture.defer_cleanup)
    })
}

fn atomic_diagnostic_retain_staging(staging: tempfile::TempDir) {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        if let Some(capture) = slot.borrow_mut().as_mut() {
            capture.deferred_staging.push(staging);
        }
    });
}

fn atomic_diagnostic_hold_retired_root(path: &Path) {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(capture) = slot.as_mut() else {
            return;
        };
        let (file, before, open_errno) = match std::fs::File::open(path) {
            Ok(file) => match file.metadata() {
                Ok(metadata) => (Some(file), metadata_identity(metadata), 0),
                Err(error) => (
                    Some(file),
                    AtomicDiagnosticIdentity {
                        raw_errno: error.raw_os_error().unwrap_or(-1),
                        ..AtomicDiagnosticIdentity::default()
                    },
                    0,
                ),
            },
            Err(error) => (
                None,
                AtomicDiagnosticIdentity::default(),
                error.raw_os_error().unwrap_or(-1),
            ),
        };
        capture.retired_roots.push(AtomicDiagnosticRetiredRoot {
            file,
            before,
            open_errno,
        });
    });
}

fn diagnostic_timestamp(capture: &AtomicDiagnosticCapture) -> u64 {
    capture.started.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

fn path_identity(path: &Path) -> AtomicDiagnosticIdentity {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => AtomicDiagnosticIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            valid: true,
            raw_errno: 0,
        },
        Err(error) => AtomicDiagnosticIdentity {
            raw_errno: error.raw_os_error().unwrap_or(-1),
            ..AtomicDiagnosticIdentity::default()
        },
    }
}

fn atomic_diagnostic_begin(
    kind: AtomicDiagnosticKind,
    target_identity: AtomicDiagnosticIdentity,
    staged_identity: AtomicDiagnosticIdentity,
) -> Option<usize> {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let capture = slot.as_mut()?;
        let started_ns = diagnostic_timestamp(capture);
        capture.events.push(AtomicDiagnosticEvent {
            kind,
            started_ns,
            ended_ns: 0,
            succeeded: 0,
            raw_errno: 0,
            target_identity,
            staged_identity,
        });
        Some(capture.events.len() - 1)
    })
}

fn atomic_diagnostic_begin_exchange(target: &Path, staged: &Path) -> Option<usize> {
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let capture = slot.as_mut()?;
        let target_identity = path_identity(target);
        let staged_identity = path_identity(staged);
        let started_ns = diagnostic_timestamp(capture);
        capture.events.push(AtomicDiagnosticEvent {
            kind: AtomicDiagnosticKind::DirectoryExchange,
            started_ns,
            ended_ns: 0,
            succeeded: 0,
            raw_errno: 0,
            target_identity,
            staged_identity,
        });
        Some(capture.events.len() - 1)
    })
}

fn atomic_diagnostic_finish(index: Option<usize>, succeeded: bool, raw_errno: Option<i32>) {
    let Some(index) = index else { return };
    ATOMIC_DIAGNOSTIC_EVENTS.with(|slot| {
        if let Some(capture) = slot.borrow_mut().as_mut() {
            let ended_ns = diagnostic_timestamp(capture);
            if let Some(event) = capture.events.get_mut(index) {
                event.ended_ns = ended_ns;
                event.succeeded = if succeeded { 1 } else { -1 };
                event.raw_errno = raw_errno.unwrap_or(0);
            }
        }
    });
}

fn atomic_diagnostic_event(kind: AtomicDiagnosticKind) {
    let index = atomic_diagnostic_begin(
        kind,
        AtomicDiagnosticIdentity::default(),
        AtomicDiagnosticIdentity::default(),
    );
    atomic_diagnostic_finish(index, true, None);
}

/// Filesystem guards: snapshots, destination validation, and ownership.
#[path = "generate_guards.rs"]
pub(crate) mod guards;

/// Preserve repository-owned entries while replacing generated output.
#[path = "generate_preserve.rs"]
mod preserve;

/// Stage and publish fresh preview output transactionally.
#[path = "generate_preview.rs"]
mod preview;

/// Own the private persistent spare used only for in-place generation.
#[path = "generate_stage_root.rs"]
mod stage_root;

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use guards::ToolSnapshot;

use guards::GenerateOwnership;
use leaf_links::check_no_symlink_or_emitted_leaf;

/// Options for [`generate`].
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Preview root; `None` replaces the in-repo `.github` tree.
    pub output_dir: Option<PathBuf>,
}

/// Report for [`generate`].
#[derive(Debug, Clone)]
pub struct GenerateReport {
    /// Repository-relative files written, sorted.
    pub files_written: Vec<String>,
    /// Recommendations shared with `plan`.
    pub recommendations: Vec<String>,
    /// Pinned validators that accepted the staged tree, sorted.
    pub validated_by: Vec<String>,
    /// Per-workspace profile provenance.
    pub profiles: Vec<ProfileProvenance>,
    /// Cleanup warnings on a committed success (empty when clean).
    pub warnings: Vec<String>,
}

/// Render in memory, then replace `.github` or write a preview.
///
/// In-place replacement stages under the root and commits, leaving the
/// previous tree unchanged on failure; previews write `PATH/.github`.
///
/// # Errors
///
/// Returns profile, render, actionlint, preview, unsafe-path, or IO
/// errors; the profile gate runs before any write, leaving outputs untouched.
pub fn generate(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
) -> Result<GenerateReport, OrchestratorError> {
    generate_dispatched(prep, opts, None)
}

/// [`generate`] with an explicit dispatch mode overriding `execution.mode`.
///
/// # Errors
///
/// Same as [`generate`], plus routing errors.
pub fn generate_dispatched(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
    dispatch: Option<ExecutionMode>,
) -> Result<GenerateReport, OrchestratorError> {
    fail_on_blocking_findings(prep)?;
    let tools = ToolSnapshot::capture(&prep.root);
    let tofu_roots = crate::select_tofu::tofu_selected_roots(&prep.discovery.statuses);
    let tofu_locks = velnor_actions_tofu::TofuLockSnapshot::capture(&prep.root, &tofu_roots);
    let tree = render_staged_tree_with(prep, dispatch)?;
    let validated_by = validate_staged(&tree)?;
    tools.verify(&prep.root)?;
    tofu_locks
        .verify(&prep.root)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let warnings = match &opts.output_dir {
        None => replace_in_place(prep, &tree)?,
        Some(dir) => preview::write_preview(prep, dir, &tree)?,
    };
    Ok(GenerateReport {
        files_written: tree.paths(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
        profiles: profile_provenance(prep),
        warnings,
    })
}

/// Fail closed on transient-only profile evidence before touching `.github`.
fn fail_on_blocking_findings(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let blockers = crate::evidence::blocking_findings(&prep.discovery.workspaces);
    if !blockers.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "{}: {}",
                velnor_actions_rust::TRANSIENT_EVIDENCE_CODE,
                blockers.join("; ")
            ),
        });
    }
    if !prep.lock_audit_blocking.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "lock_audit_blocked: {}",
                prep.lock_audit_blocking.join("; ")
            ),
        });
    }
    Ok(())
}

/// Render the validated tree (base files plus release extras) in memory.
///
/// No writes, no validators.
///
/// # Errors
///
/// Returns lock, render, actionlint, or unsafe-path errors.
pub fn render_staged_tree(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    render_staged_tree_with(prep, None)
}

/// [`render_staged_tree`] with a dispatch-mode override.
///
/// # Errors
///
/// Same as [`render_staged_tree`].
pub fn render_staged_tree_with(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let owned = owned_preparation(prep)?;
    let tree = render_all(&owned, dispatch)?;
    check_tree_paths(&tree)?;
    Ok(tree)
}

/// Render base files plus release extras into the marker-checked tree, in memory only.
fn render_all(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, dispatch).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let mut render_context = prep.workflow.context.clone();
    render_context.scale_set_selector = scale_set_selector_for_dispatch(&prep.config, dispatch)?;
    let mut actionlint_input = prep.workflow.actionlint.clone();
    actionlint_input.extra_runner_labels = render_context
        .scale_set_selector
        .as_ref()
        .map(|selector| selector.labels().to_vec())
        .unwrap_or_default();
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &render_context,
        &mise,
    )?;
    let workflow = rendered.yaml;
    let actionlint = render_actionlint_yaml(&actionlint_input)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise)?;
    extra.extend(crate::binary_release_emit::files(prep)?);
    extra.extend(crate::freshness_emit::freshness_files(prep)?);
    extra.extend(crate::owned_tool_publication::files(prep)?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep)?);
    extra.extend(crate::routing::extra_files(&prep.config, version)?);
    extra.extend(rendered.shared);
    let mut tree = render_tree_with_extra(&workflow, &actionlint, &extra, version)?;
    crate::evidence::mbx_producer_passthrough::append_allowlisted(&prep.root, &mut tree)?;
    Ok(tree)
}

/// Validate every rendered path lexically plus symlink-prefix probing.
fn check_tree_paths(tree: &RenderedTree) -> Result<Vec<SafeTreePath>, OrchestratorError> {
    let mut safe = Vec::with_capacity(tree.files.len());
    for file in &tree.files {
        safe.push(guard::validate_tree_path(&file.path)?);
    }
    for link in &tree.symlinks {
        guard::validate_tree_path(&link.path)?;
    }
    Ok(safe)
}

/// Fail when a path exists as a symlink.
fn reject_symlink(path: &Path) -> Result<(), OrchestratorError> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.is_symlink()
    {
        return Err(OrchestratorError::UnsafePath {
            path: path.display().to_string(),
            reason: "symlink_refused".to_owned(),
        });
    }
    Ok(())
}

/// Symlink-prefix guard with emitted-leaf replacement.
#[path = "generate_leaf_links.rs"]
mod leaf_links;

/// Stage under the root, then commit under the ownership lock.
///
/// Nothing before the commit touches old output; success may carry
/// old-tree-removal warnings.
fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
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
    _root: &Path,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    if !target.exists() {
        return match std::fs::rename(staged, target) {
            Ok(()) => Ok(Vec::new()),
            Err(err) => Err(OrchestratorError::io(label, err.to_string())),
        };
    }
    swap_existing_directory(target, staged, exchange_directories)
}

/// Exchange an existing target before cleaning up its old tree.
fn swap_existing_directory(
    target: &Path,
    staged: &Path,
    exchange: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
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
