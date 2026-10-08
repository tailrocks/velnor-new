//! Bounded blocking wrappers for durable controller admission control.

use std::path::Path;
use std::time::Instant;

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::time::{Instant as TokioInstant, timeout_at};

use velnor_runner_journal::journal::Journal;

/// Failure to open the existing journal or create a safe blocking runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ControlOpenError {
    /// The existing journal could not be opened without creating or migrating it.
    #[error("journal unavailable")]
    JournalUnavailable,
    /// A blocking control operation cannot run from inside another Tokio runtime.
    #[error("blocking runtime unavailable")]
    RuntimeUnavailable,
}

/// Result of requesting the durable host-wide admission fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainRequestOutcome {
    /// The drain bit was persisted and read back before the absolute deadline.
    Requested,
    /// The deadline expired before the drain mutation was dispatched.
    DeadlineBeforeMutation,
    /// The request may have committed, but its outcome could not be confirmed.
    UnknownAfterMutation,
}

/// Result of a bounded local drain wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// Every local cleanup and ownership proof was reconciled before the deadline.
    Drained,
    /// The absolute deadline expired before a complete result was observed.
    Deadline,
    /// The controller cannot prove local quiescence from current evidence.
    Unknown(DrainUnknown),
}

/// Reason a drain wait cannot establish quiescence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainUnknown {
    /// The retained service state-directory capability could not be inspected.
    StateDirectoryUnavailable,
    /// The existing journal could not be opened or read.
    JournalUnavailable,
    /// The blocking wrapper could not safely create or use its Tokio runtime.
    RuntimeUnavailable,
    /// No durable drain fence was observed.
    AdmissionNotFenced,
    /// A complete authoritative ownership inventory and cleanup proof is not available.
    OwnershipInventoryUnavailable,
}

/// Why a resume request was refused without changing the drain fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeBlockReason {
    /// No authoritative quiescence proof is available.
    QuiescenceProofUnavailable,
}

/// Result of explicitly reopening controller admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOutcome {
    /// The drain fence was cleared after a complete quiescence proof.
    Resumed,
    /// The deadline expired before a resume mutation was dispatched.
    DeadlineBeforeMutation,
    /// A resume mutation may have committed but its outcome was not confirmed.
    UnknownAfterMutation,
    /// The request was refused before mutation because its preconditions are absent.
    Blocked(ResumeBlockReason),
}

/// Persist the drain fence and confirm it under one absolute deadline.
///
/// This function never creates a journal or runs schema migration. Once the
/// write is dispatched, any error, timeout, or late read-back is reported as
/// `UnknownAfterMutation`; callers must not retry automatically.
///
/// # Errors
///
/// Returns [`ControlOpenError`] only when the runtime or existing journal is
/// unavailable before the drain mutation begins.
pub fn request_drain_blocking(
    journal_path: &Path,
    deadline: Instant,
) -> Result<DrainRequestOutcome, ControlOpenError> {
    if Instant::now() >= deadline {
        return Ok(DrainRequestOutcome::DeadlineBeforeMutation);
    }
    let runtime = blocking_runtime()?;
    let tokio_deadline = TokioInstant::from_std(deadline);

    runtime.block_on(async {
        let journal = match timeout_at(tokio_deadline, Journal::open_existing(journal_path)).await {
            Ok(Ok(journal)) => journal,
            Ok(Err(_)) => return Err(ControlOpenError::JournalUnavailable),
            Err(_) => return Ok(DrainRequestOutcome::DeadlineBeforeMutation),
        };
        if Instant::now() >= deadline {
            return Ok(DrainRequestOutcome::DeadlineBeforeMutation);
        }

        // From this point, even a timeout is ambiguous: the transaction may
        // have committed while its acknowledgement was delayed.
        if !matches!(
            timeout_at(tokio_deadline, journal.request_drain()).await,
            Ok(Ok(()))
        ) {
            return Ok(DrainRequestOutcome::UnknownAfterMutation);
        }
        if Instant::now() >= deadline {
            return Ok(DrainRequestOutcome::UnknownAfterMutation);
        }
        match timeout_at(tokio_deadline, journal.draining()).await {
            Ok(Ok(true)) if Instant::now() < deadline => Ok(DrainRequestOutcome::Requested),
            _ => Ok(DrainRequestOutcome::UnknownAfterMutation),
        }
    })
}

/// Wait for local cleanup without inferring quiescence from journal rows.
///
/// Until a complete host ownership inventory can be reconciled with durable
/// cleanup proofs, this returns `Unknown(OwnershipInventoryUnavailable)` after
/// it confirms that the admission fence is persisted.
#[must_use]
pub fn wait_drained_blocking(
    journal_path: &Path,
    _docker_endpoint: &str,
    deadline: Instant,
) -> DrainOutcome {
    if Instant::now() >= deadline {
        return DrainOutcome::Deadline;
    }
    let runtime = match blocking_runtime() {
        Ok(runtime) => runtime,
        Err(ControlOpenError::JournalUnavailable) => {
            return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable);
        }
        Err(ControlOpenError::RuntimeUnavailable) => {
            return DrainOutcome::Unknown(DrainUnknown::RuntimeUnavailable);
        }
    };
    let tokio_deadline = TokioInstant::from_std(deadline);
    runtime.block_on(async {
        let journal = match timeout_at(tokio_deadline, Journal::open_readonly(journal_path)).await {
            Ok(Ok(journal)) => journal,
            Ok(Err(_)) => return DrainOutcome::Unknown(DrainUnknown::JournalUnavailable),
            Err(_) => return DrainOutcome::Deadline,
        };
        if Instant::now() >= deadline {
            return DrainOutcome::Deadline;
        }
        match timeout_at(tokio_deadline, journal.draining()).await {
            Err(_) => DrainOutcome::Deadline,
            Ok(Err(_)) => DrainOutcome::Unknown(DrainUnknown::JournalUnavailable),
            Ok(Ok(false)) => DrainOutcome::Unknown(DrainUnknown::AdmissionNotFenced),
            Ok(Ok(true)) if Instant::now() >= deadline => DrainOutcome::Deadline,
            Ok(Ok(true)) => DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable),
        }
    })
}

/// Refuse to clear the drain fence until authoritative quiescence is proven.
///
/// The current source has no complete inventory-and-cleanup proof adapter, so
/// this validates the existing journal and then returns a blocked result
/// without calling `Journal::resume`.
///
/// # Errors
///
/// Returns [`ControlOpenError`] when the journal or blocking runtime is
/// unavailable before any resume mutation.
pub fn resume_blocking(
    journal_path: &Path,
    _docker_endpoint: &str,
    deadline: Instant,
) -> Result<ResumeOutcome, ControlOpenError> {
    if Instant::now() >= deadline {
        return Ok(ResumeOutcome::DeadlineBeforeMutation);
    }
    let runtime = blocking_runtime()?;
    let tokio_deadline = TokioInstant::from_std(deadline);
    runtime.block_on(async {
        match timeout_at(tokio_deadline, Journal::open_readonly(journal_path)).await {
            Ok(Ok(_journal)) if Instant::now() < deadline => Ok(ResumeOutcome::Blocked(
                ResumeBlockReason::QuiescenceProofUnavailable,
            )),
            Ok(Ok(_)) | Err(_) => Ok(ResumeOutcome::DeadlineBeforeMutation),
            Ok(Err(_)) => Err(ControlOpenError::JournalUnavailable),
        }
    })
}

pub(super) fn blocking_runtime() -> Result<Runtime, ControlOpenError> {
    if Handle::try_current().is_ok() {
        return Err(ControlOpenError::RuntimeUnavailable);
    }
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ControlOpenError::RuntimeUnavailable)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use tokio::runtime::Builder;

    use velnor_runner_journal::journal::Journal;

    use super::{
        ControlOpenError, DrainOutcome, DrainRequestOutcome, DrainUnknown, ResumeBlockReason,
        ResumeOutcome, request_drain_blocking, resume_blocking, wait_drained_blocking,
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Result<Self, String> {
            let path = std::env::temp_dir().join(format!(
                "velnor-launch-control-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn journal(&self) -> PathBuf {
            self.0.join("launch.db")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    fn initialize(path: &Path) -> Result<(), String> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime
            .block_on(Journal::open(path))
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn draining(path: &Path) -> Result<bool, String> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let journal = runtime
            .block_on(Journal::open_readonly(path))
            .map_err(|error| error.to_string())?;
        runtime
            .block_on(journal.draining())
            .map_err(|error| error.to_string())
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(3)
    }

    #[test]
    fn request_persists_the_fence_but_wait_stays_unknown_without_inventory_proof()
    -> Result<(), String> {
        let scratch = Scratch::new()?;
        let path = scratch.journal();
        initialize(&path)?;

        assert_eq!(
            request_drain_blocking(&path, deadline()),
            Ok(DrainRequestOutcome::Requested)
        );
        assert!(draining(&path)?);
        assert_eq!(
            wait_drained_blocking(&path, "unix:///var/run/docker.sock", deadline()),
            DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable)
        );
        assert!(
            draining(&path)?,
            "waiting must not clear the admission fence"
        );
        Ok(())
    }

    #[test]
    fn expired_request_never_dispatches_the_mutation() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let path = scratch.journal();
        initialize(&path)?;

        assert_eq!(
            request_drain_blocking(&path, Instant::now()),
            Ok(DrainRequestOutcome::DeadlineBeforeMutation)
        );
        assert!(!draining(&path)?);
        Ok(())
    }

    #[test]
    fn resume_is_blocked_and_preserves_the_drain_fence() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let path = scratch.journal();
        initialize(&path)?;
        assert_eq!(
            request_drain_blocking(&path, deadline()),
            Ok(DrainRequestOutcome::Requested)
        );

        assert_eq!(
            resume_blocking(&path, "unix:///var/run/docker.sock", deadline()),
            Ok(ResumeOutcome::Blocked(
                ResumeBlockReason::QuiescenceProofUnavailable
            ))
        );
        assert!(draining(&path)?, "a blocked resume must not clear drain");
        Ok(())
    }

    #[test]
    fn missing_journal_is_never_created_by_control_commands() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let path = scratch.journal();

        assert_eq!(
            request_drain_blocking(&path, deadline()),
            Err(ControlOpenError::JournalUnavailable)
        );
        assert_eq!(
            wait_drained_blocking(&path, "unix:///var/run/docker.sock", deadline()),
            DrainOutcome::Unknown(DrainUnknown::JournalUnavailable)
        );
        assert_eq!(
            resume_blocking(&path, "unix:///var/run/docker.sock", deadline()),
            Err(ControlOpenError::JournalUnavailable)
        );
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn blocking_api_rejects_nested_tokio_runtime_before_mutation() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let path = scratch.journal();
        initialize(&path)?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;

        let nested = runtime.block_on(async { request_drain_blocking(&path, deadline()) });
        assert_eq!(nested, Err(ControlOpenError::RuntimeUnavailable));
        assert!(!draining(&path)?);
        Ok(())
    }
}
