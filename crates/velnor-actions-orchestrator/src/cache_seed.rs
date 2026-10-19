//! Dependency seed wiring: seeded roots, crate-job restores, lane records.
//!
//! A seeded root is a lockful Cargo workspace root that owns at least one
//! Rust crate job. Its dependency seed is restored read-only by those crate
//! jobs and written only by the generated Cache seed workflow, whose lanes
//! replay the exact manifest records of the root's build, Clippy, and doc
//! tasks (selected by execution digest).

use std::collections::BTreeMap;

use velnor_actions_actionlint::PinnedActionRef;
use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};
use velnor_actions_contract::workflow::TaskExecutionManifestEntryV1;
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SeedLane, plan_output_expr, rsdeps_key_output, rsdeps_restore_keys_output,
};
use velnor_actions_contract::{Job, Step};
use velnor_actions_workflow_renderer::steps::{SeedCacheUse, seed_cache_step};

use crate::OrchestratorError;
use crate::crate_job_ids::group_runnable;
use crate::discover::Discovery;

#[path = "cache_seed_job.rs"]
pub(crate) mod job;
#[path = "cache_seed_steps.rs"]
mod steps;

/// Pinned `actions/cache/restore` or `actions/cache/save` ref.
/// # Errors
///
/// Returns a contract error when the catalog pin is rejected.
pub(crate) fn cache_uses(restore: bool) -> Result<String, OrchestratorError> {
    Ok(PinnedActionRef::new(
        "actions/cache",
        Some(if restore { "restore" } else { "save" }),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value())
}

/// Lockful roots that own a runnable Rust crate job, in `fetch_roots` order.
/// # Errors
///
/// Returns an internal error when the proposal graph is malformed.
pub(crate) fn seed_roots(
    discovery: &Discovery,
    fetch_roots: &[String],
) -> Result<Vec<String>, OrchestratorError> {
    let grouped = group_runnable(&discovery.proposals)?;
    let mut owned = std::collections::BTreeSet::new();
    for tasks in grouped.values() {
        if tasks.iter().any(|task| crate::crate_jobs::is_rust(task))
            && let Some(first) = tasks.first()
        {
            let root = crate::source_prep::workspace_root_for_manifest(
                &first.identity.unit_path,
                fetch_roots,
            );
            if fetch_roots.iter().any(|candidate| candidate == root) {
                owned.insert(root.to_owned());
            }
        }
    }
    Ok(fetch_roots
        .iter()
        .filter(|root| owned.contains(root.as_str()))
        .cloned()
        .collect())
}

/// Sources restore plus, for a seeded root, its read-only dependency seed.
/// # Errors
///
/// Returns contract or render errors for rejected pins, roots, or keys.
pub(crate) fn crate_restores(
    fetch_roots: &[String],
    seed_roots: &[String],
    manifest: &str,
    use_rust: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps: Vec<Step> = crate::source_cache::restore_step_for_crate(fetch_roots, use_rust)?
        .into_iter()
        .collect();
    if !use_rust {
        return Ok(steps);
    }
    let root = crate::source_prep::workspace_root_for_manifest(manifest, fetch_roots);
    if let Some(ordinal) = seed_roots.iter().position(|seeded| seeded == root) {
        steps.push(seed_cache_step(
            &cache_uses(true)?,
            CacheFamily::Rsdeps,
            root,
            SeedCacheUse::Restore,
            &plan_output_expr(&rsdeps_key_output(ordinal)),
            Some(&plan_output_expr(&rsdeps_restore_keys_output(ordinal))),
        )?);
    }
    Ok(steps)
}

/// Everything the Cache seed workflow is built from, fixed at workflow build.
///
/// Attach completes it per generation mode: a consumer or lock-backed
/// helper `acquire` step, or the pre-seed helper (download from the
/// triggering run, plus the helper dependency seed).
#[derive(Debug, Clone, Default)]
pub(crate) struct SeedSpec {
    /// Key-step inputs of the seed jobs (the Plan's, `tofu_roots` cleared).
    pub(crate) keys: crate::cache_keys::CacheKeyInputs,
    /// The Plan job's complete key-step inputs (attach rebuilds its step).
    pub(crate) plan_keys: crate::cache_keys::CacheKeyInputs,
    /// Per root ordinal: the lane records the seed replays.
    pub(crate) lanes: Vec<BTreeMap<SeedLane, Vec<LaneRecord>>>,
    /// Per root ordinal: the root's crate jobs compile through MBX.
    pub(crate) mbx: Vec<bool>,
    /// Runner label shared with the crate jobs.
    pub(crate) label: String,
    /// Literal seed job timeout (`None` only in the empty spec).
    pub(crate) timeout: Option<velnor_actions_contract::JobTimeout>,
    /// Staged helper version (`$RUNNER_TEMP/velnor/bin/velnor-actions-<v>`).
    pub(crate) helper_version: String,
    /// Renderer version bound by the task-execution manifest marker.
    pub(crate) generator_version: String,
    /// Default branch whose generations the janitor prunes.
    pub(crate) default_branch: String,
    /// `CARGO_BUILD_JOBS` of every lane (the compiler process budget).
    pub(crate) compiler_jobs: u32,
    /// Raw-byte ceiling of one saved generation.
    pub(crate) ceiling: u64,
    /// Helper acquisition inserted after checkout (consumer or lock mode).
    pub(crate) acquire: Option<Step>,
    /// Pre-seed mode: the release target the helper manifest must name.
    pub(crate) preseed_target: Option<String>,
    /// The workflow build's tool catalog (project-resolved version refs).
    pub(crate) catalog: Option<velnor_actions_mise::ToolCatalog>,
}

impl SeedSpec {
    /// True when no workspace root is seeded.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.seed.roots.is_empty()
    }

    /// The workflow build's catalog.
    /// # Errors
    ///
    /// Returns a contract error for the empty spec.
    pub(crate) fn catalog(&self) -> Result<&velnor_actions_mise::ToolCatalog, OrchestratorError> {
        self.catalog
            .as_ref()
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "seed_spec_without_catalog".to_owned(),
            })
    }

    /// One seed job per seeded root, keyed by job ID.
    /// # Errors
    ///
    /// Returns contract or render errors for rejected roots, pins, or steps.
    pub(crate) fn jobs(&self) -> Result<BTreeMap<String, Job>, OrchestratorError> {
        let catalog = self.catalog()?;
        let mut jobs = BTreeMap::new();
        for (ordinal, root) in self.keys.seed.roots.iter().enumerate() {
            let mut job = job::seed_job(self, catalog, ordinal)?;
            if let Some(target) = &self.preseed_target {
                let staged = format!(
                    "{}{}",
                    velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX,
                    self.helper_version
                );
                job.steps.splice(
                    1..1,
                    crate::attach::seed_preseed_consumers(target, &staged)?,
                );
            }
            jobs.insert(job::seed_job_id(root), job);
        }
        Ok(jobs)
    }
}

/// Render `cache-seed.yml` from the attached spec and prove the cache
/// budget (I8) over the finalized CI and seed jobs.
/// # Errors
///
/// Returns contract or render errors for rejected jobs or documents, or
/// when the projected hot set exceeds 85% of the configured quota.
pub(crate) fn render_seed(
    prep: &crate::prepare::GenerationPreparation,
    mise: &velnor_actions_workflow_renderer::setup::MiseSetup,
    ir: &velnor_actions_contract::WorkflowIr,
    context: &velnor_actions_workflow_renderer::render::RenderContext,
) -> Result<Option<velnor_actions_workflow_renderer::RenderedFile>, OrchestratorError> {
    use velnor_actions_workflow_renderer::{cache_budget, cache_seed_document};
    let spec = &prep.workflow.seed;
    let (file, seed_jobs) = if spec.is_empty() {
        (None, BTreeMap::new())
    } else {
        let (file, jobs) =
            cache_seed_document::render_cache_seed_workflow(cache_seed_document::CacheSeedSpec {
                ci_workflow_name: &prep.config.workflow.name,
                jobs: spec.jobs()?,
                mise,
                checkout_uses: &context.checkout_uses,
                generator_version: &spec.generator_version,
            })?;
        (Some(file), jobs)
    };
    let ci_jobs = velnor_actions_workflow_renderer::finalize_jobs(
        ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        context,
        mise,
    )?;
    cache_budget::check(
        &[&ci_jobs, &seed_jobs],
        &cache_budget::CacheBudget {
            quota_bytes: prep.config.workflow.cache_quota_bytes,
            seed_max_bytes: prep.config.workflow.seed_max_bytes,
        },
    )?;
    Ok(file)
}

/// Workflow-build inputs of [`seed_spec`].
pub(crate) struct SeedSpecInputs<'a> {
    pub(crate) catalog: &'a velnor_actions_mise::ToolCatalog,
    pub(crate) config: &'a velnor_actions_contract::VelnorConfig,
    pub(crate) label: &'a str,
    pub(crate) keys: &'a crate::cache_keys::CacheKeyInputs,
    pub(crate) built: &'a crate::crate_jobs::CrateBuild,
    pub(crate) acquire: Option<&'a Step>,
    pub(crate) helper_version: &'a str,
    pub(crate) generator_version: &'a str,
    pub(crate) default_branch: &'a str,
}

/// Fix the Cache seed inputs from the built crate jobs.
/// # Errors
///
/// Returns contract errors for an invalid timeout or record digest.
pub(crate) fn seed_spec(input: &SeedSpecInputs<'_>) -> Result<SeedSpec, OrchestratorError> {
    if input.keys.seed.roots.is_empty() {
        return Ok(SeedSpec::default());
    }
    let mut keys = input.keys.clone();
    keys.tofu_roots.clear();
    let mut lanes = Vec::new();
    let mut mbx = Vec::new();
    for root in &keys.seed.roots {
        let nested = crate::cache_keys::nested_roots(root, &keys.seed.workspace_roots);
        let records = lane_records(&input.built.jobs, root, &nested)?;
        mbx.push(records.values().flatten().any(|record| record.mbx));
        lanes.push(records);
    }
    let workflow = &input.config.workflow;
    Ok(SeedSpec {
        keys,
        plan_keys: input.keys.clone(),
        lanes,
        mbx,
        label: input.label.to_owned(),
        timeout: Some(velnor_actions_contract::JobTimeout::new(
            workflow.seed_timeout_minutes,
        )?),
        helper_version: input.helper_version.to_owned(),
        generator_version: input.generator_version.to_owned(),
        default_branch: input.default_branch.to_owned(),
        compiler_jobs: input.config.resources.compiler_process_budget,
        ceiling: workflow.seed_max_bytes,
        acquire: input.acquire.cloned(),
        preseed_target: None,
        catalog: Some(input.catalog.clone()),
    })
}

/// One task record a seed lane replays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaneRecord {
    /// Stable manifest task ID.
    pub(crate) task_id: String,
    /// Manifest execution digest the lane selects the record by.
    pub(crate) execution_digest: String,
    /// The record uses the Nextest runner.
    pub(crate) nextest: bool,
    /// The record compiles through MBX.
    pub(crate) mbx: bool,
}

/// Build, Clippy, and doc records owned by `root`, sorted by task ID.
///
/// Mirrors the key step's record digest exactly: the task kind is the
/// second-to-last task-ID segment and the owner is the root containing the
/// record's `--manifest-path`, excluding nested workspace roots.
/// # Errors
///
/// Returns a contract error if a record digest cannot be computed.
pub(crate) fn lane_records(
    jobs: &[(String, Job)],
    root: &str,
    nested: &[String],
) -> Result<BTreeMap<SeedLane, Vec<LaneRecord>>, OrchestratorError> {
    let mut lanes: BTreeMap<SeedLane, BTreeMap<String, LaneRecord>> = BTreeMap::new();
    for step in jobs.iter().flat_map(|(_, job)| &job.steps) {
        let Some(record) =
            TaskExecutionManifestEntryV1::from_step_kind(&step.kind).map_err(|err| {
                OrchestratorError::Contract {
                    problem: err.to_string(),
                }
            })?
        else {
            continue;
        };
        let mut segments = record.task_id.rsplit('/');
        let kind = segments.nth(1).unwrap_or_default();
        let Some(lane) = SeedLane::for_task_kind(kind) else {
            continue;
        };
        if !manifest_of(&record.argv).is_some_and(|manifest| owned(manifest, root, nested)) {
            continue;
        }
        lanes.entry(lane).or_default().insert(
            record.task_id.clone(),
            LaneRecord {
                nextest: record.argv.iter().any(|arg| arg == "nextest"),
                mbx: record
                    .argv
                    .iter()
                    .skip_while(|arg| arg.as_str() != "--")
                    .nth(1)
                    .is_some_and(|program| program == "mbx"),
                task_id: record.task_id,
                execution_digest: record.execution_digest,
            },
        );
    }
    Ok(lanes
        .into_iter()
        .map(|(lane, records)| (lane, records.into_values().collect()))
        .collect())
}

fn manifest_of(argv: &[String]) -> Option<&str> {
    let at = argv.iter().position(|arg| arg == "--manifest-path")?;
    argv.get(at + 1).map(String::as_str)
}

fn owned(manifest: &str, root: &str, nested: &[String]) -> bool {
    (root.is_empty() || manifest.starts_with(&format!("{root}/")))
        && !nested
            .iter()
            .any(|other| manifest.starts_with(&format!("{other}/")))
}

#[cfg(test)]
#[path = "cache_seed_tests.rs"]
mod tests;
