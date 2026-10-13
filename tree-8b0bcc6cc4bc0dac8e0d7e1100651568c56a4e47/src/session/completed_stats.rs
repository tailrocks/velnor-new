//! Explicit supported completed statistics; legacy report shape stays separate.
use super::{cache_misses, incremental_compilations};
use crate::session::completed_report::{SessionIdentity, WorkloadResult, publish};
use eyre::Result;
use mbx_cache_core::{AgentStats, CompletedMeasurement, LinkAttribution, MeasurementCoverage};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Cache-owned counters, distinct from actual subprocess work and task results.
#[derive(Debug, Serialize)]
pub(crate) struct CompletedCacheStatistics {
    /// Owning cache-session lifetime, not the workload execution interval.
    pub session_duration_ns: u64,
    /// Exclusive cumulative wrapper phases, not additive workload-wall slots.
    pub wrapper_phases_ns: BTreeMap<String, u64>,
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub unconsulted: u64,
    pub incremental_compilations: u64,
    pub verifications: u64,
    pub divergences: u64,
    pub bypasses: BTreeMap<String, u64>,
    pub predictions_loaded: u64,
    pub prefetched_actions: u64,
    /// Historical estimated avoided work, never measured workload savings.
    pub estimated_compiler_duration_avoided_ns: u64,
    /// Remote object payload bytes; excludes external CI archive transfers.
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub stored_bytes: u64,
    pub restored_output_files: u64,
    pub restored_output_bytes: u64,
    pub background_uploads: u64,
    pub background_upload_failures: u64,
    pub remote_failures: u64,
    pub upload_drain_duration_ns: u64,
    pub remote_manifest_lookup_duration_ns: u64,
    pub remote_action_lookup_duration_ns: u64,
    pub remote_blob_transfer_duration_ns: u64,
    pub local_cas_write_duration_ns: u64,
    pub materialization_duration_ns: u64,
}

impl From<&AgentStats> for CompletedCacheStatistics {
    fn from(stats: &AgentStats) -> Self {
        Self {
            session_duration_ns: stats.session_duration_ns,
            wrapper_phases_ns: stats.wrapper_phases_ns.clone(),
            lookups: stats.lookups,
            hits: stats.hits,
            misses: cache_misses(stats),
            unconsulted: stats.unconsulted,
            incremental_compilations: incremental_compilations(stats),
            verifications: stats.verifications,
            divergences: stats.divergences,
            bypasses: stats.bypasses.clone(),
            predictions_loaded: stats.predictions_loaded,
            prefetched_actions: stats.prefetched_actions,
            estimated_compiler_duration_avoided_ns: stats.avoided_compiler_duration_ns,
            downloaded_bytes: stats.downloaded_bytes,
            uploaded_bytes: stats.uploaded_bytes,
            stored_bytes: stats.stored_bytes,
            restored_output_files: stats.restored_output_files,
            restored_output_bytes: stats.restored_output_bytes,
            background_uploads: stats.background_uploads,
            background_upload_failures: stats.background_upload_failures,
            remote_failures: stats.remote_failures,
            upload_drain_duration_ns: stats.upload_drain_duration_ns,
            remote_manifest_lookup_duration_ns: stats.remote_manifest_lookup_duration_ns,
            remote_action_lookup_duration_ns: stats.remote_action_lookup_duration_ns,
            remote_blob_transfer_duration_ns: stats.remote_blob_transfer_duration_ns,
            local_cas_write_duration_ns: stats.local_cas_write_duration_ns,
            materialization_duration_ns: stats.materialization_duration_ns,
        }
    }
}

/// Versioned completed report payload, produced directly from owning counters.
#[derive(Debug, Serialize)]
pub(crate) struct CompletedStatistics<'a> {
    pub measurement: CompletedMeasurement,
    pub cache: CompletedCacheStatistics,
    /// Actual owning Cargo stream capture, or explicit unavailable evidence.
    pub cargo_capture: Option<&'a crate::cargo_artifact_capture::CargoCaptureReport>,
    /// Native bootstrap/route identity; external source qualification is separate.
    pub native_dispatch: Option<&'a crate::dispatch_identity::NativeDispatchWitness>,
    /// Original native closed-admission inventory; no parsed report remints it.
    pub native_admission_closure: Option<&'a crate::dispatch_admission::AdmissionClosure>,
    /// Native local integrity; external source qualification remains separate.
    pub local_qualification: Option<crate::session::measurement_qualification::Qualification>,
}

/// Publish immutable, directly measured statistics when directory mode is enabled.
/// Missing timing intervals remain absent; publication errors preserve callers'
/// ability to report the original workload outcome.
pub(crate) fn publish_completed_stats(
    directory: Option<&Path>,
    identity: &SessionIdentity,
    workload: WorkloadResult,
    workload_wall_ns: Option<u64>,
    cache_post_workload_drain_ns: Option<u64>,
    cargo_capture: Option<&crate::cargo_artifact_capture::CargoCaptureReport>,
    native_dispatch: Option<&crate::dispatch_identity::NativeDispatchWitness>,
    admission_closure: Option<&crate::dispatch_admission::AdmissionClosure>,
    stats: &AgentStats,
) -> Result<Option<PathBuf>> {
    let Some(directory) = directory else {
        return Ok(None);
    };
    let mut statistics = CompletedStatistics {
        measurement: CompletedMeasurement {
            coverage: MeasurementCoverage::default(),
            measurement_package_generation: stats.measurement_package_generation,
            measurement_package_availability: stats.measurement_package_availability,
            measurement_packages: stats.measurement_packages.clone(),
            workload_wall_ns,
            cache_post_workload_drain_ns,
            adapters: stats.measurement_adapters.clone(),
            link_wall_ns: None,
            link_attribution: LinkAttribution::CombinedUnknown,
        },
        cache: CompletedCacheStatistics::from(stats),
        cargo_capture,
        native_dispatch,
        native_admission_closure: admission_closure,
        local_qualification: None,
    };
    statistics.local_qualification =
        Some(crate::session::measurement_qualification::evaluate_native(
            directory,
            identity,
            &statistics.measurement,
            cargo_capture,
            native_dispatch,
            admission_closure,
        ));
    if let Some(qualification) = &statistics.local_qualification {
        statistics.measurement.coverage = qualification.coverage;
    }
    publish(directory, identity, workload, &statistics).map(Some)
}

#[cfg(test)]
#[path = "completed_stats_tests.rs"]
mod tests;
