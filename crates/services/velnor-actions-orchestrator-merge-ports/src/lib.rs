//! Merge/cover contract: shared vocabulary plus the cover port.
//!
//! This crate sits at the bottom of the orchestrator family: it owns the
//! data both `merge` and `cover` pass across their seam (request,
//! manifests, signals, sinks, shard proofs, schema version) and the
//! [`CoverPort`] trait through which merge calls cover behavior. Neither
//! side depends on the other; the cover crate implements the port.

mod changed_work;
mod cover_baseline_port;
mod cover_port;
mod cover_types;
mod merge_types;
mod schema;
mod scoped_compare;
mod scoped_compare_join;
mod shard_types;

pub use changed_work::{changed_keys, member_changed};
pub use cover_baseline_port::CoverBaselinePort;
pub use cover_port::CoverPort;
pub use cover_types::{CoverSinks, Fold, Partition, Signals};
pub use merge_types::{
    BaselineManifest, BaselineTaskEntry, CandidateAttestation, MergeRequest, TaskReportArtifactId,
    TaskReportCheckRunId, TaskReportOutputFanIn, TaskReportOutputOrigin, TaskReportProducerOutput,
};
pub use schema::{SCHEMA, check_schema};
pub use scoped_compare::{
    ActionsAttemptArtifactView, ActionsAttemptJobView, CompleteActionsAttemptView,
    ScopedCompareError, ScopedCompareLane, ScopedCompareLaneBinding, ScopedCompareRequest,
    ScopedCompareResult,
};
pub use scoped_compare_join::bind_scoped_compare;
pub use shard_types::{ResourceLimits, ShardProof, TestIdentity, inventory_digest};
