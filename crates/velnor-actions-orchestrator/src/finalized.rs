use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, ReleaseManifest, TargetRecord, WorkflowPolicy, expand_workflow,
};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::attach::{attach_final_lock_acquire, attach_preseed};
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::validate::verify_velnor_repository_files;

pub(crate) fn owned_preparation(
    prep: &GenerationPreparation,
) -> Result<GenerationPreparation, OrchestratorError> {
    let mut owned = prep.clone();
    if prep.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
        let lock = verify_velnor_repository_files(&prep.root)?.ok_or_else(|| {
            OrchestratorError::Contract {
                problem: "required_verdict_requires_published_generator_lock".to_owned(),
            }
        })?;
        validate_published_generator_lock(&lock)?;
        attach_preseed(
            &mut owned.workflow,
            &lock,
            &prep.runner_label,
            env!("CARGO_PKG_VERSION"),
            &prep.emission_catalog(),
        )?;
        attach_final_lock_acquire(
            &mut owned.workflow.ir,
            &lock,
            &prep.runner_label,
            env!("CARGO_PKG_VERSION"),
        )?;
    }
    Ok(owned)
}

fn validate_published_generator_lock(
    lock: &velnor_actions_contract::GeneratorLock,
) -> Result<(), OrchestratorError> {
    let manifest = ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: lock.generator.version.clone(),
        repository: velnor_actions_contract::targets::EXPECTED_REPOSITORY.to_owned(),
        commit: lock.generator.commit.clone(),
        targets: lock
            .generator
            .binaries
            .iter()
            .map(|binary| TargetRecord {
                target: binary.target.clone(),
                artifact: binary.artifact.clone(),
                sha256: binary.sha256.clone(),
            })
            .collect(),
    };
    manifest
        .validate(".velnor/generator.lock")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    lock.check_supported_targets(".velnor/generator.lock")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    velnor_actions_mise::catalog::lock::verify_lock_against_manifest(lock, &manifest).map_err(
        |err| OrchestratorError::Contract {
            problem: err.to_string(),
        },
    )
}

pub fn finalized_jobs(
    prep: &GenerationPreparation,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let owned = owned_preparation(prep)?;
    let mise = resolve_mise_setup(&owned.config, &owned.runner_label)?;
    let ir = expand_workflow(&owned.workflow.ir, &owned.config, None).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(finalize_render_jobs(
        &ir,
        owned.config.workflow.policy,
        owned.workflow.support.as_ref(),
        &owned.workflow.context,
        &mise,
    )?)
}
