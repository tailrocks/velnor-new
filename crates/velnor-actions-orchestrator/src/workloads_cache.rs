//! Read only native source consumers; dedicated pure producers own export.

use std::collections::BTreeMap;

#[path = "workloads_cache_bun.rs"]
pub(crate) mod bun;
#[path = "workloads_cache_bun_source_job.rs"]
pub(crate) mod bun_source_job;
#[path = "workloads_cache_docker.rs"]
pub(crate) mod docker;
#[path = "workloads_cache_gradle.rs"]
mod gradle;
#[path = "workloads_cache_npm.rs"]
mod npm;
#[path = "workloads_cache_npm_proof.rs"]
pub(crate) mod npm_proof;
#[path = "workloads_cache_npm_sanitize.rs"]
mod npm_sanitize;
#[path = "workloads_cache_npm_source_job.rs"]
pub(crate) mod npm_source_job;
#[path = "workloads_cache_source_report.rs"]
pub(crate) mod source_report;
#[path = "workloads_cache_source_snapshot.rs"]
pub(crate) mod source_snapshot;

/// Closed task IDs select an isolated download owner without exposing config.
pub(crate) fn task_env(task_id: &str) -> BTreeMap<String, String> {
    if !task_id.starts_with("stack/workload/") {
        return BTreeMap::new();
    }
    match task_id.rsplit('/').next() {
        Some("bun_ci") => bun::task_env(),
        Some("gradle_check" | "gradle_database_check") => gradle::task_env(),
        Some("node_ci") => npm::task_env(),
        _ => BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "workloads_cache_tests.rs"]
mod tests;
