//! Actual subprocess measurements, separate from cache outcomes and estimates.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Adapter owning a measured invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    /// Rust compiler.
    Rustc,
    /// C or C++ compiler.
    Cc,
    /// Execution of a Cargo build script.
    BuildScript,
    /// Rust documentation tool.
    Rustdoc,
}

/// Whether an invocation performs build work or queries tool identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    /// Build or documentation work.
    Work,
    /// Tool discovery or configuration probe.
    Probe,
}

/// One terminal cache disposition for an invocation, independent of processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOutcome {
    /// A valid cached result was restored.
    Hit,
    /// A lookup did not restore a result.
    Miss,
    /// No usable lookup identity was available.
    Unconsulted,
    /// The invocation declined caching.
    Bypass,
    /// Cached output was rebuilt for verification.
    Verification,
    /// The invocation did not establish a terminal disposition.
    Unknown,
}

/// Purpose of a real child process, independent of its cache disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPurpose {
    /// Actual compilation, build-script execution or documentation generation.
    Work,
    /// Actual tool identity/configuration query.
    Probe,
    /// Rustdoc finalization, including finalization after a cache hit.
    RustdocFinalize,
}

/// Operating-system observation of an attempted child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOutcome {
    /// A child exited successfully.
    Succeeded,
    /// A child exited with a failure code.
    Failed,
    /// A child terminated without an exit code.
    Terminated,
    /// No child was started.
    SpawnFailed,
    /// A child started but its terminal outcome was not observed.
    WaitFailed,
}

/// Package ownership from authoritative Cargo resolution, never a label guess.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageOrigin {
    /// Member of the owning Cargo workspace.
    Workspace,
    /// Cargo registry package.
    Registry,
    /// Cargo Git package.
    Git,
    /// Resolved local path package outside the workspace members.
    Path,
    /// Authoritative package provenance is unavailable.
    #[default]
    Unknown,
}

/// Real Cargo unit/package identity, with unavailable components explicit.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitIdentity {
    /// Cargo's unit hash, supplied in the actual compiler invocation.
    pub cargo_unit_id: Option<String>,
    /// Full package ID supplied by Cargo resolution, when available.
    pub package_id: Option<String>,
    /// Resolved package ownership; absent evidence remains unknown.
    pub origin: PackageOrigin,
}

/// Cumulative actual child observations; unknown walls are not zero samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessMeasurement {
    /// Attempted spawns, including failed spawns.
    pub attempts: u64,
    /// Children successfully started.
    pub started: u64,
    /// Sum of observed spawn-to-terminal wall intervals in nanoseconds.
    pub observed_wall_ns: u64,
    /// Number of observed wall intervals included in the sum.
    pub wall_observations: u64,
}

/// Invocation and child-process aggregates for one unit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitMeasurement {
    /// Authoritative unit identity, or an explicit unknown bucket.
    pub identity: Option<UnitIdentity>,
    /// One cache outcome for each wrapper invocation.
    pub invocations: BTreeMap<InvocationKind, BTreeMap<CacheOutcome, u64>>,
    /// Actual processes, including probes and finalization after cache hits.
    pub subprocesses: BTreeMap<ProcessPurpose, BTreeMap<ProcessOutcome, ProcessMeasurement>>,
}

/// Totals and unit attribution for one adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterMeasurement {
    /// One cache outcome for each wrapper invocation.
    pub invocations: BTreeMap<InvocationKind, BTreeMap<CacheOutcome, u64>>,
    /// Actual processes, independent of the wrapper's cache outcome.
    pub subprocesses: BTreeMap<ProcessPurpose, BTreeMap<ProcessOutcome, ProcessMeasurement>>,
    /// Unit-level totals; unknown identity is retained explicitly.
    pub units: Vec<UnitMeasurement>,
    /// Attribution rows omitted by a supported bound; totals still continue.
    pub omitted_unit_events: u64,
}

/// A directly recorded observation sent from an adapter to its owning agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementEvent {
    /// A single wrapper invocation completed, including unknown disposition.
    Invocation {
        /// Owning adapter.
        adapter: AdapterKind,
        /// Work or probe.
        invocation_kind: InvocationKind,
        /// The invocation's terminal cache disposition.
        cache_outcome: CacheOutcome,
        /// Authoritative Cargo attribution, when available.
        unit: Option<UnitIdentity>,
    },
    /// A single attempted child process reached an observed boundary.
    Process {
        /// Owning adapter.
        adapter: AdapterKind,
        /// Work, probe or rustdoc finalization.
        purpose: ProcessPurpose,
        /// Actual operating-system outcome.
        outcome: ProcessOutcome,
        /// Exactly one attempt, with wall absence represented by its count.
        measurement: ProcessMeasurement,
        /// Authoritative Cargo attribution, when available.
        unit: Option<UnitIdentity>,
    },
}

/// Attribution available for time spent in linker subprocesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkAttribution {
    /// Linker work remains included in parent process wall, without a split.
    CombinedUnknown,
}

/// Processes to which a completeness claim applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementScope {
    /// MBX-owned adapter invocations; never every compiler on the host.
    MbxOwnedAdapters,
}

/// Strength of evidence that every in-scope observation was delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCoverageStatus {
    /// No positive completeness proof exists; absent records cannot prove zero.
    Unknown,
    /// A delivery failure or unmatched attempted observation was established.
    Unverified,
    /// Positive native enrollment and delivery-completeness proof was checked.
    Verified,
}

/// Bounded reason a supported measurement cannot establish complete coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCoverageReason {
    /// No positive native completeness proof has been established.
    CompletenessNotProven,
    /// Delivery of an in-scope observation failed.
    DeliveryFailed,
    /// A native delivery attempt has no matching acknowledgment.
    DeliveryIncomplete,
}

/// Explicit measurement scope and reliability, independent of workload success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementCoverage {
    /// Scope covered by the evidence.
    pub scope: MeasurementScope,
    /// Unknown and unverified totals cannot establish zero actual process work.
    pub status: MeasurementCoverageStatus,
    /// Reason unavailable completeness is retained explicitly.
    pub reason: Option<MeasurementCoverageReason>,
}

impl Default for MeasurementCoverage {
    fn default() -> Self {
        Self {
            scope: MeasurementScope::MbxOwnedAdapters,
            status: MeasurementCoverageStatus::Unknown,
            reason: Some(MeasurementCoverageReason::CompletenessNotProven),
        }
    }
}

/// Supported completed-session measurement payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedMeasurement {
    /// Reliability scope; a completed session alone does not prove delivery.
    pub coverage: MeasurementCoverage,
    /// Observed owning workload wall; absent when no terminal boundary exists.
    pub workload_wall_ns: Option<u64>,
    /// Post-workload cache drain; absent when its boundaries are unknown.
    pub cache_post_workload_drain_ns: Option<u64>,
    /// Actual invocation/process totals and attribution by adapter.
    pub adapters: BTreeMap<AdapterKind, AdapterMeasurement>,
    /// Separate linker wall measurement; currently unavailable.
    pub link_wall_ns: Option<u64>,
    /// Explicit reason linker work cannot be separated.
    pub link_attribution: LinkAttribution,
}
