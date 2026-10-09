use super::admission::{PolicyGap, PolicyMismatch};

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
    /// The configured Docker endpoint did not yield a bounded logical Engine identity.
    DockerEngineUnavailable,
    /// The source reports incomplete but non-contradictory policy evidence.
    PoolUnknown(PolicyGap),
    /// The source reports a mismatch with the immutable configured policy.
    PoolRejected(PolicyMismatch),
    /// A same-scope verified policy and session capability are available.
    VerifiedSessionReady,
    /// The verified capability has no repository-scoped durable close permit.
    SessionCloseUnsupportedScope,
    /// Session creation or a later protocol effect may have occurred.
    SessionEffectUncertain,
    /// A prior durable singleton session prevents another session create.
    ExistingSessionHeld,
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
    /// A whole Available message remains unacknowledged because one or more
    /// offers were not durably processed without uncertainty.
    AvailableOffersHeld,
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
