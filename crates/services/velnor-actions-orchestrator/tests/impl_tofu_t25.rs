//! T25 waste-removal pins: hoisted read-cache construction sites and
//! exactly-once plan-artifact downloads per job.
//!
//! The shared [`FileCache`](velnor_actions_tofu_core::FileCache) must be
//! constructed once per top-level phase (never per call inside the
//! tofu adapter), and every generated job must download the plan
//! artifact at most once (runner isolation keeps the per-job
//! download; only duplicates within a job are waste).

use velnor_actions_orchestrator::{finalized_jobs, prepare};
use velnor_actions_workflow_jobs::context::PUBLISH_JOB_ID;

use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::tofu_repo;
use crate::impl_tofu_t24_gates::{crate_src, is_crate_job, token_hits};
use crate::support::TestResult;

/// Hoisted constructions only: one `FileCache` per top-level phase.
///
/// `discover` owns the prepare-phase instance (detection, inventory,
/// diagnostics share it), `build_plan` owns the plan-phase instance
/// (every task shares it), and cover owns one for its closure plus
/// extension check. The tofu adapter itself never constructs one:
/// per-call fresh caches would share nothing.
#[test]
fn file_cache_constructions_are_hoisted_per_phase() -> TestResult {
    let orch = crate_src("");
    let disc = crate_src("../velnor-actions-orchestrator-discovery");
    let cover = crate_src("../velnor-actions-orchestrator-cover-baseline");
    let internal = crate_src("../velnor-actions-orchestrator-internal");
    let mut sites: Vec<String> = Vec::new();
    for (dir, prefix) in [
        (&orch, ""),
        (&disc, "discovery/"),
        (&cover, "cover-baseline/"),
        (&internal, "internal/"),
    ] {
        sites.extend(
            token_hits(dir, "FileCache::new()")?
                .into_iter()
                .filter(|hit| !hit.starts_with("cover_identity_fixtures.rs:"))
                .map(|hit| format!("{prefix}{hit}")),
        );
    }
    sites.sort();
    let files: Vec<&str> = sites
        .iter()
        .map(|hit| {
            hit.rsplit('/')
                .next()
                .and_then(|base| base.split_once(':'))
                .map_or("", |(name, _)| name)
        })
        .collect();
    assert_eq!(
        files,
        ["cover_identity.rs", "discover.rs", "internal.rs"],
        "one hoisted construction per phase (the cfg(test) fixtures \
         companion is excluded): {sites:?}"
    );
    let tofu = crate_src("../../adapters/velnor-actions-tofu");
    assert!(
        token_hits(&tofu, "FileCache::new()")?.is_empty(),
        "tofu adapter never constructs per-call caches"
    );
    assert!(
        token_hits(&tofu, "FileCache::default()")?.is_empty(),
        "no default-constructed cold caches either"
    );
    Ok(())
}

/// Every generated job downloads the plan artifact at most once.
///
/// Runner isolation keeps the per-job download (each hosted job runs
/// on its own runner), so cross-job repetition is intentional; only
/// a second download within one job would be waste. Crate jobs and
/// the publish job each carry exactly one.
#[test]
fn plan_artifact_downloads_exactly_once_per_job() -> TestResult {
    let dir = tofu_repo(2)?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let mut crate_jobs = 0;
    for (id, job) in &jobs {
        let downloads = job
            .steps
            .iter()
            .filter(|step| step.name == "Download plan")
            .count();
        assert!(
            downloads <= 1,
            "{id} downloads the plan at most once: {downloads}"
        );
        if is_crate_job(id) {
            crate_jobs += 1;
            assert_eq!(downloads, 1, "{id} carries its plan download");
        }
    }
    assert_eq!(crate_jobs, 2, "two tofu stack jobs");
    let publish = jobs.get(PUBLISH_JOB_ID).ok_or("publish job renders")?;
    assert_eq!(
        publish
            .steps
            .iter()
            .filter(|step| step.name == "Download plan")
            .count(),
        1,
        "publish carries its plan download"
    );
    Ok(())
}
