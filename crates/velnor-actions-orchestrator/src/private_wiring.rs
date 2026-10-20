//! Wiring of the private-source block into generated Rust jobs (HD-7).
//!
//! A job whose lock closure is non-empty gets the mint, fetch and revoke steps
//! immediately before its Cargo source fetch, that is, before any
//! repository-controlled step: the token is minted for the exact allowlist,
//! used by the Velnor-owned fetch, and revoked before the obligations run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use velnor_actions_contract::config::PrivateSourcesConfig;
use velnor_actions_contract::deps_lock::DepsLock;
use velnor_actions_contract::{Job, Step, StepRole, VelnorConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::private_source_steps::{
    PrivateSourcesSpec, private_source_steps,
};

use crate::OrchestratorError;
use crate::config::config_error;
use crate::config::deps_lock::read_deps_lock;
use crate::config::private_sources::{SourceKind, job_allowlist};
use crate::internal::internal;

/// Toolchain keys the fetch's Cargo run needs from the generated step env.
const OWNED_KEYS: [&str; 9] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
    "MISE_RUSTUP_HOME",
    "MISE_CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
];

/// Source kinds a Rust job retrieves; git trees and Maven bundles belong to the
/// jobs that consume them.
const RUST_JOB_KINDS: [SourceKind; 2] = [SourceKind::Cargo, SourceKind::ReleaseAssets];

/// Source kinds a Bun member job retrieves: pinned release-asset bundles. Cargo
/// sources, git trees and Maven bundles are not Bun inputs.
const BUN_JOB_KINDS: [SourceKind; 1] = [SourceKind::ReleaseAssets];

/// Everything needed to render the block for any set of workspace roots.
#[derive(Debug)]
pub(crate) struct PrivateWiring {
    config: PrivateSourcesConfig,
    lock: Option<DepsLock>,
    workspace: PathBuf,
    toolchain_env: BTreeMap<String, String>,
}

impl PrivateWiring {
    /// Load the wiring for a repository; `None` without `[private_sources]`.
    ///
    /// # Errors
    /// A missing lock for declared sources, or a lock that drifted from the
    /// declarations, fails generation (the freshness gate for the lock).
    pub(crate) fn load(
        workspace: &Path,
        config: &VelnorConfig,
        catalog: &ToolCatalog,
    ) -> Result<Option<Self>, OrchestratorError> {
        let Some(private) = &config.private_sources else {
            return Ok(None);
        };
        if config.workflow.policy != velnor_actions_contract::WorkflowPolicy::ConsumerV1 {
            // Only consumer workflows stage the helper the private operations need.
            return Err(OrchestratorError::config(
                ".velnor/config.toml",
                "private_sources",
                "requires_consumer_policy",
            ));
        }
        let lock = read_deps_lock(workspace)?;
        match &lock {
            Some(lock) => lock
                .check_matches(private, velnor_actions_contract::config::DEPS_LOCK_PATH)
                .map_err(config_error)?,
            None if private.source_ids().next().is_some() => {
                return Err(internal("deps_lock_missing"));
            }
            None => {}
        }
        let all = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
        let toolchain_env = all
            .into_iter()
            .filter(|(key, _)| OWNED_KEYS.contains(&key.as_str()))
            .collect();
        Ok(Some(Self {
            config: private.clone(),
            lock,
            workspace: workspace.to_owned(),
            toolchain_env,
        }))
    }

    /// The block for a job covering `roots`; empty when nothing is private.
    ///
    /// # Errors
    /// Lock read and render-spec failures.
    pub(crate) fn steps_for_roots(&self, roots: &[String]) -> Result<Vec<Step>, OrchestratorError> {
        self.steps_for_kinds(roots, &RUST_JOB_KINDS)
    }

    /// The block for a Bun member job rooted at `root`; empty without release assets.
    ///
    /// # Errors
    /// Lock read and render-spec failures.
    pub(crate) fn steps_for_bun(&self, root: &str) -> Result<Vec<Step>, OrchestratorError> {
        self.steps_for_kinds(&[root.to_owned()], &BUN_JOB_KINDS)
    }

    fn steps_for_kinds(
        &self,
        roots: &[String],
        kinds: &[SourceKind],
    ) -> Result<Vec<Step>, OrchestratorError> {
        let kinds: BTreeSet<SourceKind> = kinds.iter().copied().collect();
        let repositories = job_allowlist(
            &self.workspace,
            &self.config,
            self.lock.as_ref(),
            roots,
            &kinds,
        )?;
        if repositories.is_empty() {
            return Ok(Vec::new());
        }
        let kind_names: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
        let root_names: Vec<String> = roots
            .iter()
            .map(|root| {
                if root.is_empty() {
                    ".".to_owned()
                } else {
                    root.clone()
                }
            })
            .collect();
        let steps = private_source_steps(&PrivateSourcesSpec {
            owner: &self.config.owner,
            app_id_variable: &self.config.app_id_variable,
            app_private_key_secret: &self.config.app_private_key_secret,
            repositories: &repositories,
            kinds: &kind_names,
            roots: &root_names,
            toolchain_env: &self.toolchain_env,
        })
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
        Ok(steps.into())
    }

    /// Insert the block before the job's first Cargo source fetch.
    ///
    /// Jobs that stage the helper later (the plan job) get it moved up to just
    /// before the block, so the internal operations have their binary.
    ///
    /// A job with no Cargo roots (a Bun-only repository) fetches no Cargo
    /// source, and release assets are consumed by the jobs that name them, so
    /// it gets no block.
    ///
    /// # Errors
    /// Fails when the job has Cargo roots but no Cargo source fetch to precede.
    pub(crate) fn insert_into_job(
        &self,
        job: &mut Job,
        roots: &[String],
    ) -> Result<(), OrchestratorError> {
        if roots.is_empty() {
            return Ok(());
        }
        let block = self.steps_for_roots(roots)?;
        if block.is_empty() {
            return Ok(());
        }
        let at = job
            .steps
            .iter()
            .position(|step| step.role == Some(StepRole::CargoSourcesFetch))
            .ok_or_else(|| internal("private_sources_without_cargo_sources_step"))?;
        let acquire_at = job
            .steps
            .iter()
            .position(|step| step.role == Some(StepRole::AcquireVelnor));
        let mut inserted = block;
        if let Some(index) = acquire_at
            && index > at
        {
            inserted.insert(0, job.steps.remove(index));
        }
        job.steps.splice(at..at, inserted);
        Ok(())
    }
}

#[cfg(test)]
#[path = "private_wiring_tests.rs"]
mod tests;
