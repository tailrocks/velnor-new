//! Staged `.github` replacement and outside-repo preview writes.
//! In-place commits use atomic directory exchange on supported platforms and
//! fail closed when an existing tree cannot be exchanged.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract::{ExecutionMode, expand_workflow};
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::RenderedTree;
use velnor_actions_workflow_renderer::steps::rehead_actionlint_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_extra;

use crate::OrchestratorError;
use crate::finalized::owned_preparation;
use crate::pins::{resolve_mise_setup, resolve_mise_setup_set};
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
#[must_use]
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
                Some(Ok(metadata)) => metadata_identity(&metadata),
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

fn metadata_identity(metadata: &std::fs::Metadata) -> AtomicDiagnosticIdentity {
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
                Ok(metadata) => (Some(file), metadata_identity(&metadata), 0),
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
    u64::try_from(capture.started.elapsed().as_nanos()).unwrap_or(u64::MAX)
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
include!("generate.part2.rs");
