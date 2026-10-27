// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use velnor_actions_contract::config::PrivateSourcesConfig;
use velnor_actions_contract::deps_lock::DepsLock;
use velnor_actions_contract::{
    Job, PRIVATE_FETCH_TOKEN_ENV, PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepKind, StepRole, VelnorConfig,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::cache_sources::{
    ImmutableSourceObject, is_approved_origin, is_never_archive_path, require_approved_origins,
};
use velnor_actions_workflow_renderer::private_source_steps::{
    PrivateSourcesSpec, private_source_steps,
};

use crate::OrchestratorError;
use crate::config::config_error;
use crate::config::deps_lock::read_deps_lock;
use crate::config::private_sources::{SourceKind, job_allowlist};
use crate::internal::internal;

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

const RUST_JOB_KINDS: [SourceKind; 2] = [SourceKind::Cargo, SourceKind::ReleaseAssets];

const BUN_JOB_KINDS: [SourceKind; 1] = [SourceKind::ReleaseAssets];

#[derive(Debug)]
pub(crate) struct PrivateWiring {
    config: PrivateSourcesConfig,
    lock: Option<DepsLock>,
    workspace: PathBuf,
    toolchain_env: BTreeMap<String, String>,
}

impl PrivateWiring {
    pub(crate) fn load(
        workspace: &Path,
        config: &VelnorConfig,
        catalog: &ToolCatalog,
    ) -> Result<Option<Self>, OrchestratorError> {
        let Some(private) = &config.private_sources else {
            return Ok(None);
        };
        if config.workflow.policy != velnor_actions_contract::WorkflowPolicy::ConsumerV1 {
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
        check_private_asset_layout(private, lock.as_ref())?;
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

    pub(crate) fn steps_for_roots(&self, roots: &[String]) -> Result<Vec<Step>, OrchestratorError> {
        self.steps_for_kinds(roots, &RUST_JOB_KINDS)
    }

    pub(crate) fn steps_for_bun(&self, root: &str) -> Result<Vec<Step>, OrchestratorError> {
        self.steps_for_kinds(&[root.to_owned()], &BUN_JOB_KINDS)
    }

    pub(crate) fn release_asset_pins(&self) -> Vec<String> {
        let mut pins: Vec<String> = self
            .lock
            .iter()
            .flat_map(|lock| &lock.release_assets)
            .map(|asset| format!("{}:{}", asset.id, asset.sha256))
            .collect();
        pins.sort();
        pins
    }

    pub(crate) fn steps_for_gradle(
        &self,
        roots: &[String],
    ) -> Result<Vec<Step>, OrchestratorError> {
        self.steps_for_kinds(roots, &[SourceKind::MavenBundles])
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
        verify_private_block(&steps)?;
        Ok(steps.into())
    }

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

pub(crate) fn verify_private_block(steps: &[Step]) -> Result<(), OrchestratorError> {
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    if roles
        != [
            Some(StepRole::PrivateTokenMint),
            Some(StepRole::PrivateFetch),
            Some(StepRole::PrivateRevoke),
        ]
    {
        return Err(internal("private_block_order"));
    }
    for step in steps {
        match &step.kind {
            StepKind::Action { with, env, .. } => {
                if with
                    .values()
                    .any(|value| value.contains(PRIVATE_TOKEN_OUTPUT_EXPR))
                {
                    return Err(internal("private_token_output_exposure"));
                }
                check_private_env(env)?;
            }
            StepKind::Shell { env, .. }
            | StepKind::TaskExecution { env, .. }
            | StepKind::Internal { env, .. } => check_private_env(env)?,
            StepKind::NativeCheckExecution { .. } => {}
        }
    }
    Ok(())
}

fn check_private_env(env: &BTreeMap<String, String>) -> Result<(), OrchestratorError> {
    for (key, value) in env {
        if key == PRIVATE_FETCH_TOKEN_ENV {
            if value != PRIVATE_TOKEN_OUTPUT_EXPR {
                return Err(internal("private_token_binding"));
            }
        } else if value.contains(PRIVATE_TOKEN_OUTPUT_EXPR) {
            return Err(internal("private_token_output_exposure"));
        }
    }
    Ok(())
}

pub(crate) fn check_private_asset_layout(
    config: &PrivateSourcesConfig,
    lock: Option<&DepsLock>,
) -> Result<(), OrchestratorError> {
    let Some(lock) = lock else {
        return Ok(());
    };
    let mut approved: Vec<&str> = Vec::new();
    for source in config
        .maven_bundles
        .iter()
        .chain(config.release_assets.iter())
    {
        approved.push(source.repository.as_str());
    }
    for tree in &config.git_trees {
        approved.push(tree.repository.as_str());
    }
    let mut objects: Vec<ImmutableSourceObject> = Vec::new();
    for bundle in &lock.maven_bundles {
        stable_private_path(&bundle.id)?;
        objects.push(private_asset_object(
            &bundle.id,
            &bundle.repository,
            &bundle.sha256,
        ));
    }
    for asset in &lock.release_assets {
        let Some(relative) = asset.extract_to.as_deref() else {
            return Err(internal("private_asset_without_extract_to"));
        };
        stable_private_path(relative)?;
        objects.push(private_asset_object(
            &asset.id,
            &asset.repository,
            &asset.sha256,
        ));
    }
    for tree in &lock.git_trees {
        stable_private_path(&tree.id)?;
        if !is_approved_origin(&tree.repository, &approved) {
            return Err(internal(&format!(
                "private_tree_origin_unapproved:{}",
                tree.id
            )));
        }
    }
    require_approved_origins(&objects, &approved).map_err(|error| map_mise(&error))?;
    Ok(())
}

fn private_asset_object(id: &str, repository: &str, sha256: &str) -> ImmutableSourceObject {
    ImmutableSourceObject {
        object_id: id.to_owned(),
        kind: "private_asset".to_owned(),
        approved_origin_id: repository.to_owned(),
        content_digest: format!("s256-{sha256}"),
    }
}

fn stable_private_path(path: &str) -> Result<(), OrchestratorError> {
    crate::source_prep::cache_source_projection::validate_relative_path(path)?;
    if is_never_archive_path(path) {
        return Err(internal("private_asset_credential_path"));
    }
    Ok(())
}

fn map_mise(error: &velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}
