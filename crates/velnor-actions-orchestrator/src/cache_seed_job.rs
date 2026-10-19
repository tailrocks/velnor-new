//! One Cache seed job per seeded workspace root.
//!
//! The job prepares exactly what the root's crate jobs prepare (tools, Rust
//! components, sources, the install-only MBX trio, the same job env), runs
//! the same `Compute cache keys` step, and skips every remaining step when
//! the exact dependency seed already exists. Otherwise it restores the
//! previous generation by prefix (warm seed), stubs member sources, replays
//! the build, Clippy, and doc lanes sequentially, trims the profiles to
//! dependency units, saves one complete generation, and deletes the rest.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RSDEPS_HELPER_KEY_STEP_EXPR, RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR,
    RSDEPS_HELPER_SAVE_CONDITION, RSDEPS_SAVE_CONDITION, SeedLane, key_step_expr,
    rsdeps_key_output, rsdeps_restore_keys_output,
};
use velnor_actions_contract::{Job, Step, StepId};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::steps::{SeedCacheUse, seed_cache_step};

use super::{SeedSpec, steps};
use crate::OrchestratorError;

const D_GATE: &str = "steps.seed-lookup.outputs.cache-hit != 'true'";
const HELPER_GATE: &str = "steps.seed-helper-lookup.outputs.cache-hit != 'true'";

/// Job ID of the seed job for `root`.
#[must_use]
pub(crate) fn seed_job_id(root: &str) -> String {
    if root.is_empty() {
        "cache-seed".to_owned()
    } else {
        format!("cache-seed-{}", root.replace(['/', '.', '_'], "-"))
    }
}

/// One root's job context shared by the step builders below.
struct RootJob<'a> {
    spec: &'a SeedSpec,
    catalog: &'a ToolCatalog,
    root: &'a str,
    ordinal: usize,
    lanes: &'a BTreeMap<SeedLane, Vec<super::LaneRecord>>,
    helper: Option<&'a Vec<String>>,
    key: String,
    restore: String,
}

/// Build one root's seed job.
/// # Errors
///
/// Returns contract or render errors for rejected roots, pins, or steps.
pub(crate) fn seed_job(
    input: &SeedSpec,
    catalog: &ToolCatalog,
    ordinal: usize,
) -> Result<Job, OrchestratorError> {
    let root = input
        .keys
        .seed
        .roots
        .get(ordinal)
        .ok_or_else(|| contract("seed_root_ordinal"))?
        .as_str();
    let no_lanes = BTreeMap::new();
    let job = RootJob {
        spec: input,
        catalog,
        root,
        ordinal,
        lanes: input.lanes.get(ordinal).unwrap_or(&no_lanes),
        helper: input
            .keys
            .seed
            .helper_build
            .as_ref()
            .filter(|_| root.is_empty()),
        key: key_step_expr(&rsdeps_key_output(ordinal)),
        restore: super::cache_uses(true)?,
    };
    let mut steps = job.prelude()?;
    steps.extend(job.gated()?);
    steps.extend(job.lanes_and_trim()?);
    steps.extend(save_pair(
        input,
        catalog,
        CacheFamily::Rsdeps,
        root,
        &job.key,
    )?);
    if job.helper.is_some() {
        steps.extend(save_pair(
            input,
            catalog,
            CacheFamily::RsdepsHelper,
            root,
            RSDEPS_HELPER_KEY_STEP_EXPR,
        )?);
    }
    Ok(Job {
        display_name: if root.is_empty() {
            "Cache seed".to_owned()
        } else {
            format!("Cache seed ({root})")
        },
        runs_on: input.label.clone(),
        check_runner: None,
        timeout_minutes: input
            .timeout
            .ok_or_else(|| contract("seed_timeout_missing"))?,
        needs: Vec::new(),
        condition: Some(seed_job_condition(&input.default_branch)),
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

impl RootJob<'_> {
    /// Skip every remaining step when each family's exact key exists.
    fn gate(&self) -> String {
        if self.helper.is_some() {
            format!("{D_GATE} || {HELPER_GATE}")
        } else {
            D_GATE.to_owned()
        }
    }

    /// Checkout, helper, the crate jobs' tool setup, keys, and exact probes.
    fn prelude(&self) -> Result<Vec<Step>, OrchestratorError> {
        let mut steps = vec![steps::checkout_step()?];
        steps.extend(self.spec.acquire.iter().cloned());
        let use_nextest = self.lanes.values().flatten().any(|record| record.nextest);
        steps.push(crate::matrix_step::prepare_crate_tools_step(
            self.catalog,
            true,
            use_nextest,
            false,
            false,
        )?);
        steps.push(crate::workflow::prepare_rust_components_step(self.catalog)?);
        steps.extend(crate::cache_keys::cache_keys_step(
            self.catalog,
            &self.spec.keys,
        )?);
        let mut lookup = self.seed_step(CacheFamily::Rsdeps, SeedCacheUse::Lookup)?;
        lookup.id = Some(StepId::SeedLookup);
        steps.push(lookup);
        if self.helper.is_some() {
            let mut lookup = self.seed_step(CacheFamily::RsdepsHelper, SeedCacheUse::Lookup)?;
            lookup.id = Some(StepId::SeedHelperLookup);
            steps.push(lookup);
        }
        Ok(steps)
    }

    /// Sources, the previous generation, MBX, fetch, and the skeleton.
    fn gated(&self) -> Result<Vec<Step>, OrchestratorError> {
        let mut gated = vec![
            crate::source_cache::seed_sources_restore()?,
            self.seed_step(CacheFamily::Rsdeps, SeedCacheUse::Restore)?,
        ];
        if self.helper.is_some() {
            gated.push(self.seed_step(CacheFamily::RsdepsHelper, SeedCacheUse::Restore)?);
        }
        if self.spec.mbx.get(self.ordinal).copied().unwrap_or(false) {
            gated.extend(crate::mbx_preflight::steps_for_catalog(self.catalog)?);
        }
        gated.extend(crate::source_prep::fetch_steps_for_crate(
            self.catalog,
            &[self.root.to_owned()],
        )?);
        gated.extend(steps::skeleton_steps(
            self.catalog,
            self.root,
            &self.spec.helper_version,
        )?);
        let gate = self.gate();
        for step in &mut gated {
            step.condition = Some(gate.clone());
        }
        Ok(gated)
    }

    /// The sequential lanes, then the trim.
    fn lanes_and_trim(&self) -> Result<Vec<Step>, OrchestratorError> {
        let mut steps = Vec::new();
        for lane in SeedLane::RSDEPS {
            let digests: Vec<String> = self
                .lanes
                .get(&lane)
                .into_iter()
                .flatten()
                .map(|record| record.execution_digest.clone())
                .collect();
            let mut step = steps::lane_step(
                lane,
                &digests,
                (&self.spec.helper_version, &self.spec.generator_version),
                self.spec.compiler_jobs,
            )?;
            step.condition = Some(D_GATE.to_owned());
            steps.push(step);
        }
        let mut lanes = SeedLane::RSDEPS.to_vec();
        if let Some(build) = self.helper {
            let mut step = steps::helper_lane_step(self.catalog, build, self.spec.compiler_jobs)?;
            step.condition = Some(HELPER_GATE.to_owned());
            steps.push(step);
            lanes.push(SeedLane::Helper);
        }
        let mut trim = steps::trim_step(
            self.root,
            &lanes,
            &self.spec.helper_version,
            self.spec.ceiling,
            (&self.key, self.helper.map(|_| RSDEPS_HELPER_KEY_STEP_EXPR)),
        )?;
        trim.condition = Some(self.gate());
        steps.push(trim);
        Ok(steps)
    }

    /// One family's exact-key lookup or prefix restore.
    fn seed_step(
        &self,
        family: CacheFamily,
        cache_use: SeedCacheUse,
    ) -> Result<Step, OrchestratorError> {
        let (key, prefix) = match family {
            CacheFamily::RsdepsHelper => (
                RSDEPS_HELPER_KEY_STEP_EXPR.to_owned(),
                RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR.to_owned(),
            ),
            _ => (
                self.key.clone(),
                key_step_expr(&rsdeps_restore_keys_output(self.ordinal)),
            ),
        };
        let prefix = (cache_use == SeedCacheUse::Restore).then_some(prefix.as_str());
        Ok(seed_cache_step(
            &self.restore,
            family,
            self.root,
            cache_use,
            &key,
            prefix,
        )?)
    }
}

/// Seed only after a same-repository push to the default branch that was
/// not cancelled (a red run still seeds: decision 1 is producer-scoped).
#[must_use]
pub(crate) fn seed_job_condition(default_branch: &str) -> String {
    format!(
        "github.event.workflow_run.event == 'push' && github.event.workflow_run.head_branch == '{default_branch}' && github.event.workflow_run.head_repository.full_name == github.repository && github.event.workflow_run.conclusion != 'cancelled'"
    )
}

/// One family's producer-gated save and its janitor.
fn save_pair(
    input: &SeedSpec,
    catalog: &ToolCatalog,
    family: CacheFamily,
    root: &str,
    key: &str,
) -> Result<[Step; 2], OrchestratorError> {
    let uses = super::cache_uses(false)?;
    let (id, condition) = match family {
        CacheFamily::RsdepsHelper => (StepId::SeedHelperSave, RSDEPS_HELPER_SAVE_CONDITION),
        _ => (StepId::SeedSave, RSDEPS_SAVE_CONDITION),
    };
    let mut save = seed_cache_step(&uses, family, root, SeedCacheUse::Save, key, None)?;
    save.id = Some(id);
    save.condition = Some(condition.to_owned());
    let janitor = steps::janitor_step(catalog, family, root, &input.default_branch, key, id)?;
    Ok([save, janitor])
}

fn contract(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
