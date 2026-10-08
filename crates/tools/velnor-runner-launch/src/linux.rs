//! Fail-closed Linux daemon coordination over one validated host snapshot.

use std::fmt;
use std::num::NonZeroU32;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use zeroize::Zeroizing;

use velnor_runner_host::worker::{DiagnosticsStore, ProtectedStateDirectory};
use velnor_runner_host::{HostPlatform, ValidatedHostConfigSnapshot};
use velnor_runner_journal::journal::Journal;

mod admission;
mod shutdown;
pub use admission::{PolicyGap, PolicyMismatch, PoolAdmissionEvidence, VerifiedPoolPolicy};
#[cfg(test)]
#[path = "linux/tests.rs"]
mod tests;

pub(super) const SIGNAL_POLL: Duration = Duration::from_millis(250);

/// Why this coordinator cannot enter a future typed offer path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxAdmissionState {
    /// The configured Ubuntu 26 selector has no source-qualified image profile.
    RunnerProfileUnavailable,
    /// The installed `AppArmor` policy does not produce the opaque profile token.
    RunnerProfileAdmissionUnavailable,
    /// The host-only credentials were unavailable to the caller.
    CredentialsUnavailable,
    /// A bounded preflight timed out or its transport/storage returned an error.
    PoolPreflightUnavailable,
    /// The source reports incomplete but non-contradictory policy evidence.
    PoolUnknown(admission::PolicyGap),
    /// The source reports a mismatch with the immutable configured policy.
    PoolRejected(admission::PolicyMismatch),
    /// Pool policy passed, but fenced population and per-offer trust are not wired.
    VerifiedOfferGateClosed,
    /// A shutdown request arrived before the admission preflight could finish.
    ShutdownBeforePreflight,
}

/// Why local shutdown could not establish a complete quiescence proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxShutdownGap {
    /// The durable admission fence was not confirmed.
    DrainFenceUnconfirmed,
    /// The journal could not be read after the fence.
    JournalUnavailable,
    /// Docker could not return a complete bounded ownership inventory.
    DockerInventoryUnavailable,
    /// At least one durable launch or credential operation remains unresolved.
    UnresolvedIntent,
    /// Velnor-labelled Docker resources remain or do not match exact rows.
    OwnedResourcesRemain,
    /// Safe diagnostics storage or exact-generation cleanup was unavailable.
    CleanupUnavailable,
    /// The final complete journal/inventory read finished after the stop cutoff.
    QuiescenceSnapshotPastDeadline,
}

/// Terminal result from the Linux daemon-owned stop coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxDaemonOutcome {
    /// Every tracked operation is resolved and the complete Docker inventory is empty.
    Quiescent {
        /// Admission evidence observed before shutdown; this is never readiness.
        admission: LinuxAdmissionState,
        /// Number of exact worker generations whose physical proof was recorded now.
        cleaned_generations: usize,
    },
    /// The finite stop cutoff elapsed before a complete quiescence proof was read.
    Deadline {
        /// Admission evidence observed before shutdown; this is never readiness.
        admission: LinuxAdmissionState,
        /// Number of launch rows that still occupy durable capacity, when readable.
        occupied_launches: Option<usize>,
        /// Number of unresolved non-launch journal operations, when readable.
        unresolved_intents: Option<usize>,
        /// Reason the cutoff elapsed without full quiescence.
        gap: LinuxShutdownGap,
        /// Number of objects in the last complete Docker inventory, if available.
        owned_resources: Option<usize>,
    },
    /// Shutdown stopped without enough evidence to report quiescence.
    Unresolved {
        /// Admission evidence observed before shutdown; this is never readiness.
        admission: LinuxAdmissionState,
        /// Reason the complete local proof is absent.
        gap: LinuxShutdownGap,
        /// Number of launch rows that still occupy durable capacity, when readable.
        occupied_launches: Option<usize>,
        /// Number of unresolved non-launch journal operations, when readable.
        unresolved_intents: Option<usize>,
        /// Number of objects in the last complete Docker inventory, if available.
        owned_resources: Option<usize>,
        /// Number of completed physical cleanup proofs recorded by this run.
        cleaned_generations: usize,
    },
}

/// State owned by one Linux daemon invocation, derived from the immutable config read.
#[derive(Debug)]
pub struct LinuxLaunchContext {
    pub(super) snapshot: ValidatedHostConfigSnapshot,
    pub(super) state_directory: PathBuf,
    pub(super) docker_endpoint: String,
    drain_timeout: Duration,
    max_jobs: NonZeroU32,
}

impl LinuxLaunchContext {
    /// Bind one validated snapshot to the protected systemd state directory.
    ///
    /// The journal is always `state_directory/launch.db`; diagnostics are
    /// opened beneath this same service-owned directory by the host facade.
    ///
    /// # Errors
    ///
    /// Returns `InvalidContext` for another platform, a relative path, a path
    /// containing parent traversal, or missing finite host limits.
    pub fn from_snapshot(
        snapshot: ValidatedHostConfigSnapshot,
        state_directory: &Path,
    ) -> Result<Self, LinuxDaemonError> {
        if snapshot.platform() != HostPlatform::Linux
            || !state_directory.is_absolute()
            || state_directory
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(LinuxDaemonError::InvalidContext);
        }
        let config = snapshot.config();
        let drain_timeout = config
            .drain_timeout_secs()
            .map(Duration::from_secs)
            .map_err(|_| LinuxDaemonError::InvalidContext)?;
        let max_jobs =
            NonZeroU32::new(config.max_jobs()).ok_or(LinuxDaemonError::InvalidContext)?;
        Ok(Self {
            docker_endpoint: config.docker.endpoint.clone(),
            state_directory: state_directory.to_path_buf(),
            snapshot,
            drain_timeout,
            max_jobs,
        })
    }

    /// Exact digest from the protected config bytes used to create this context.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        self.snapshot.policy_digest()
    }

    /// Host-wide launch ceiling read from the same validated snapshot.
    #[must_use]
    pub const fn max_jobs(&self) -> NonZeroU32 {
        self.max_jobs
    }

    /// Validated maximum shutdown interval from this immutable host snapshot.
    #[must_use]
    pub const fn drain_timeout(&self) -> Duration {
        self.drain_timeout
    }
}

/// Separate role inputs for the host PAT and Actions-read REST requests.
///
/// One configured PAT may be supplied for both roles only when its scopes are
/// authorized for their union. The short-lived Actions Service admin token is
/// never accepted here.
pub struct LinuxLaunchCredentials {
    pub(super) controller: Zeroizing<String>,
    pub(super) actions_read: Zeroizing<String>,
}

impl LinuxLaunchCredentials {
    /// Construct role-separated host credential inputs.
    ///
    /// # Errors
    ///
    /// Returns `InvalidCredentials` for an empty token or control characters.
    pub fn new(controller: String, actions_read: String) -> Result<Self, LinuxDaemonError> {
        let controller = Zeroizing::new(controller);
        let actions_read = Zeroizing::new(actions_read);
        if invalid_token(&controller) || invalid_token(&actions_read) {
            return Err(LinuxDaemonError::InvalidCredentials);
        }
        Ok(Self {
            controller,
            actions_read,
        })
    }
}

impl fmt::Debug for LinuxLaunchCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LinuxLaunchCredentials")
            .field("controller", &"[redacted]")
            .field("actions_read", &"[redacted]")
            .finish()
    }
}

/// Run the fail-closed Linux daemon loop and perform exact proof-backed cleanup on stop.
///
/// This path owns the journal, preflight transport, credential-intent store,
/// and cleanup proof adapter. It never calls the legacy Scale Set creator or
/// legacy worker starter. Even `Verified` pool policy remains blocked until a
/// fenced zero-population observation and per-offer trust path are connected.
///
/// The receiver's first `Some` cutoff is retained even when its sender closes.
/// If a prior process left the durable drain bit set, recovery uses an
/// immediate cutoff instead of silently extending the old stop window.
///
/// # Errors
///
/// Returns an error only when the initial journal cannot be opened or the
/// caller supplied an invalid context. Runtime cleanup uncertainty is returned
/// as a typed [`LinuxDaemonOutcome`] and leaves durable rows occupied.
pub async fn run_linux_daemon(
    context: LinuxLaunchContext,
    credentials: Option<LinuxLaunchCredentials>,
    protected_state: ProtectedStateDirectory,
    mut shutdown: watch::Receiver<Option<Instant>>,
) -> Result<LinuxDaemonOutcome, LinuxDaemonError> {
    let (journal, diagnostics) =
        open_runtime_state(&context.state_directory, &protected_state).await?;
    let mut cutoff = *shutdown.borrow_and_update();
    let was_draining = journal
        .draining()
        .await
        .map_err(|_| LinuxDaemonError::JournalUnavailable)?;
    if was_draining && cutoff.is_none() {
        cutoff = Some(Instant::now());
    }

    let admission = if cutoff.is_some() || was_draining {
        LinuxAdmissionState::ShutdownBeforePreflight
    } else {
        admission::prepare_admission(
            &context,
            credentials.as_ref(),
            &journal,
            &mut shutdown,
            &mut cutoff,
        )
        .await
    };

    if cutoff.is_none() {
        cutoff =
            Some(shutdown::wait_for_shutdown(&journal, &mut shutdown, context.drain_timeout).await);
    }
    let deadline = cutoff.unwrap_or_else(Instant::now);
    if !shutdown::confirm_drain(&journal, deadline).await {
        return Ok(shutdown::unresolved_outcome(
            &journal,
            admission,
            LinuxShutdownGap::DrainFenceUnconfirmed,
            0,
            None,
            deadline,
        )
        .await);
    }
    shutdown::reconcile_shutdown(&context, &journal, &diagnostics, admission, deadline).await
}

async fn open_runtime_state(
    state_directory: &Path,
    protected_state: &ProtectedStateDirectory,
) -> Result<(Journal, DiagnosticsStore), LinuxDaemonError> {
    // Use the same retained descriptor that the CLI used to create daemon.lock.
    // The journal additionally pins the leaf's parent dev/inode before bootstrap.
    let identity = protected_state
        .identity()
        .map_err(|_| LinuxDaemonError::StateDirectoryUnavailable)?;
    let journal = Journal::open_protected_at(
        &state_directory.join("launch.db"),
        identity.device(),
        identity.inode(),
    )
    .await
    .map_err(|_| LinuxDaemonError::JournalUnavailable)?;
    let diagnostics = protected_state
        .open_diagnostics_store()
        .map_err(|_| LinuxDaemonError::StateDirectoryUnavailable)?;
    Ok((journal, diagnostics))
}

/// Errors that prevent the daemon from establishing its initial trusted inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinuxDaemonError {
    /// The immutable snapshot or protected state-directory binding was invalid.
    #[error("invalid Linux launch context")]
    InvalidContext,
    /// A role-specific credential was empty or malformed.
    #[error("invalid Linux launch credential")]
    InvalidCredentials,
    /// The daemon could not validate its service-owned state directory.
    #[error("Linux service state directory unavailable or unsafe")]
    StateDirectoryUnavailable,
    /// The daemon could not securely open or initialize its durable journal.
    #[error("Linux launch journal unavailable or unsafe")]
    JournalUnavailable,
}

fn invalid_token(token: &str) -> bool {
    token.trim().is_empty() || token.chars().any(char::is_control)
}

fn deadline_after(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}
