use std::{
    path::Path,
    time::{Duration, Instant},
};

use velnor_actions_contract::{
    ObligationDecision, canonical_json_bytes, config::NativeHomebrewOperation, digest_b3,
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::formula::verify_formula;
use super::install::{InstallRequest, install_formula};
use super::{NativeCheckOutcome, NativeCheckResult};

#[derive(Clone)]
struct LoadedCheck {
    plan: velnor_actions_contract::Plan,
    entry: velnor_actions_contract::MatrixEntry,
    digest: String,
    check: velnor_actions_contract::config::NativeHomebrewCheck,
    deadline: CheckDeadline,
}

struct VerifiedCheck {
    loaded: LoadedCheck,
    obligation: velnor_actions_contract::PlanObligation,
    identity: crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
}

pub(crate) fn execute_native_homebrew_check_to(
    root: &Path,
    temp: &Path,
    run_key: &str,
    check_id: &str,
    operation: NativeHomebrewOperation,
    task_id: &str,
) -> Result<usize, OrchestratorError> {
    let started = Instant::now();
    let loaded = load_check(root, temp, run_key, check_id, task_id, started)?;
    let report_check = loaded.clone();
    let outcome = (|| {
        if loaded.check.operation != operation {
            return Err(internal("native_check_task_mismatch"));
        }
        let verified = verify_check(root, loaded, task_id)?;
        execute_verified_check(root, temp, task_id, check_id, operation, &verified)
    })();
    write_terminal_reports(temp, started, &report_check, &outcome)?;
    outcome.map(|_| 1)
}

fn load_check(
    root: &Path,
    temp: &Path,
    run_key: &str,
    check_id: &str,
    task_id: &str,
    started: Instant,
) -> Result<LoadedCheck, OrchestratorError> {
    let plan = crate::task_report::load_plan(run_key, temp)?;
    let (entry, digest) = crate::task_report::entry_and_digest(&plan, task_id)?;
    let entry = entry.clone();
    let digest = digest.to_owned();
    let config = crate::config::load_config(root)?;
    let check = config
        .native_checks
        .iter()
        .find(|item| item.id == check_id)
        .cloned()
        .ok_or_else(|| internal("native_check_not_configured"))?;
    let deadline = CheckDeadline::from_start(
        started,
        Duration::from_secs(u64::from(check.timeout_minutes) * 60),
    )
    .map_err(|error| internal(&error.to_string()))?;
    Ok(LoadedCheck {
        plan,
        entry,
        digest,
        check,
        deadline,
    })
}

fn verify_check(
    root: &Path,
    loaded: LoadedCheck,
    task_id: &str,
) -> Result<VerifiedCheck, OrchestratorError> {
    if task_id != format!("internal/native-check/{}", loaded.check.id)
        || loaded.entry.stack_id != "native"
        || loaded.entry.job_id != format!("check-{}", loaded.check.id)
    {
        return Err(internal("native_check_task_mismatch"));
    }
    let (expected_obligation, expected_entry) = rederive_plan(root, &loaded)?;
    let obligation = loaded
        .plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .cloned()
        .ok_or_else(|| internal("native_check_obligation_missing"))?;
    compare_plan_identity(
        &loaded.entry,
        &obligation,
        &expected_entry,
        &expected_obligation,
    )?;
    let identity = serde_json::from_value(
        loaded
            .entry
            .adapter_metadata
            .get("identity")
            .cloned()
            .unwrap_or_default(),
    )
    .map_err(|_| internal("native_check_identity_invalid"))?;
    Ok(VerifiedCheck {
        loaded,
        obligation,
        identity,
    })
}

fn rederive_plan(
    root: &Path,
    loaded: &LoadedCheck,
) -> Result<
    (
        velnor_actions_contract::PlanObligation,
        velnor_actions_contract::MatrixEntry,
    ),
    OrchestratorError,
> {
    let (obligation, mut entries) = crate::internal_plan::native_homebrew_checks::derive(
        root,
        &loaded.check,
        &crate::internal_plan::native_homebrew_checks::proposal(&loaded.check)
            .map_err(|_| internal("native_check_proposal_invalid"))?,
        &loaded.plan.run_key,
        &loaded.plan.generator,
        loaded.plan.event,
    )?;
    let entry = entries
        .pop()
        .ok_or_else(|| internal("native_check_matrix_missing"))?;
    Ok((obligation, entry))
}

fn compare_plan_identity(
    entry: &velnor_actions_contract::MatrixEntry,
    actual: &velnor_actions_contract::PlanObligation,
    expected_entry: &velnor_actions_contract::MatrixEntry,
    expected: &velnor_actions_contract::PlanObligation,
) -> Result<(), OrchestratorError> {
    if actual.decision != ObligationDecision::Execute
        || actual.task_digest != expected.task_digest
        || actual.input_digest != expected.input_digest
        || actual.closure_digest != expected.closure_digest
        || canonical_json_bytes(entry).map_err(|_| internal("native_check_entry_encode"))?
            != canonical_json_bytes(expected_entry)
                .map_err(|_| internal("native_check_entry_encode"))?
    {
        Err(internal("native_check_plan_identity"))
    } else {
        Ok(())
    }
}

fn execute_verified_check(
    root: &Path,
    temp: &Path,
    task_id: &str,
    check_id: &str,
    operation: NativeHomebrewOperation,
    verified: &VerifiedCheck,
) -> Result<Vec<u8>, OrchestratorError> {
    let result = match operation {
        NativeHomebrewOperation::TapTree => tap_tree_result(root, verified)?,
        NativeHomebrewOperation::FormulaContract => formula_result(root, verified)?,
        NativeHomebrewOperation::Install => {
            install_result(root, temp, task_id, check_id, verified)?
        }
        NativeHomebrewOperation::AdoptionContract => adoption_result(root, verified)?,
    };
    canonical_json_bytes(&result).map_err(|_| internal("native_check_result_encode"))
}

fn tap_tree_result(
    root: &Path,
    verified: &VerifiedCheck,
) -> Result<NativeCheckResult, OrchestratorError> {
    let tree = crate::internal_plan::native_homebrew_checks::read_checked_tap_tree(
        root,
        &verified.identity,
        verified.loaded.deadline,
    )?;
    let tree_bytes = canonical_json_bytes(
        &tree
            .iter()
            .map(|entry| (&entry.path, entry.mode, &entry.blob_sha))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| internal("native_check_tree_encode"))?;
    Ok(NativeCheckResult {
        schema: 1,
        operation: NativeHomebrewOperation::TapTree,
        identity: verified.identity.clone(),
        outcome: NativeCheckOutcome::TapTree {
            tree_digest: digest_b3(&tree_bytes),
        },
    })
}

fn formula_result(
    root: &Path,
    verified: &VerifiedCheck,
) -> Result<NativeCheckResult, OrchestratorError> {
    let channel = verified
        .loaded
        .check
        .formula_channel
        .ok_or_else(|| internal("native_check_formula_channel_missing"))?;
    let (receipt_digest, bytes) = verify_formula(
        root,
        channel,
        &verified.identity,
        verified.loaded.plan.event,
        verified.loaded.deadline,
    )?;
    Ok(NativeCheckResult {
        schema: 1,
        operation: NativeHomebrewOperation::FormulaContract,
        identity: verified.identity.clone(),
        outcome: NativeCheckOutcome::FormulaContract {
            formula_bytes: bytes,
            public_release_receipt_digest: receipt_digest,
        },
    })
}

fn install_result(
    root: &Path,
    temp: &Path,
    task_id: &str,
    check_id: &str,
    verified: &VerifiedCheck,
) -> Result<NativeCheckResult, OrchestratorError> {
    let channel = verified
        .loaded
        .check
        .formula_channel
        .ok_or_else(|| internal("native_check_formula_channel_missing"))?;
    let (evidence, receipt_digest, hosted_profile) = install_formula(InstallRequest {
        root,
        runner_temp: temp,
        channel,
        identity: &verified.identity,
        event: verified.loaded.plan.event,
        deadline: verified.loaded.deadline,
        plan: &verified.loaded.plan,
        entry: &verified.loaded.entry,
        obligation: &verified.obligation,
        entry_digest: &verified.loaded.digest,
        task_id,
        check_id,
    })?;
    Ok(NativeCheckResult {
        schema: 1,
        operation: NativeHomebrewOperation::Install,
        identity: verified.identity.clone(),
        outcome: NativeCheckOutcome::Install {
            evidence: Box::new(evidence),
            public_release_receipt_digest: receipt_digest,
            hosted_profile,
        },
    })
}

fn adoption_result(
    root: &Path,
    verified: &VerifiedCheck,
) -> Result<NativeCheckResult, OrchestratorError> {
    let event = verified.loaded.plan.event;
    let seed = crate::internal_plan::native_homebrew_checks::validate_seed_transition(
        root,
        &crate::internal_plan::native_homebrew_checks::caller_repository(root)?,
        event,
        crate::internal_plan::native_homebrew_checks::pull_request_number_from_env(event)?,
        &verified.loaded.plan.head,
        verified.loaded.deadline,
    )?;
    validate_seed_identity(&verified.identity, &seed)?;
    Ok(NativeCheckResult {
        schema: 1,
        operation: NativeHomebrewOperation::AdoptionContract,
        identity: verified.identity.clone(),
        outcome: NativeCheckOutcome::AdoptionContract {
            evidence: seed.evidence,
        },
    })
}

fn validate_seed_identity(
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    seed: &crate::internal_plan::native_homebrew_checks::SeedTransitionAdmission,
) -> Result<(), OrchestratorError> {
    let transition = seed.transition.transition();
    let expected =
        crate::internal_plan::native_homebrew_checks::NativeCheckAuthorityIdentity::Seed {
            transition_id: transition.transition_id.clone(),
            transition_digest: transition
                .digest()
                .map_err(|_| internal("homebrew_adoption_transition_invalid"))?,
            base_tree: transition.base_tree.clone(),
            sdk_source_sha: seed.sdk.source_sha.clone(),
            sdk_version: seed.sdk.version.clone(),
            sdk_manifest_sha256: seed.sdk.manifest_sha256.clone(),
            sdk_seed_runner_binary_sha256: seed.sdk.seed_runner_binary_sha256.clone(),
        };
    if identity.authority == expected {
        Ok(())
    } else {
        Err(internal("homebrew_adoption_identity_mismatch"))
    }
}

fn write_terminal_reports(
    temp: &Path,
    started: Instant,
    loaded: &LoadedCheck,
    outcome: &Result<Vec<u8>, OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let code = i32::from(u8::from(outcome.is_err()));
    let duration = u64::try_from(started.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let mut task = crate::task_report::terminal_task_report(
        &loaded.plan,
        &loaded.entry,
        &loaded.digest,
        code,
        Some(duration),
    )
    .map_err(|_| internal("native_check_task_report"))?;
    if let Ok(result_bytes) = outcome {
        write_execution_receipt(temp, loaded, result_bytes, &mut task)?;
    }
    let matrix = crate::task_report::single_task_aggregate(&loaded.plan, &loaded.entry, &task)
        .map_err(|_| internal("native_check_matrix_report"))?;
    crate::task_report::write_entry_reports(temp, &loaded.plan, &loaded.entry, &task, &matrix)
}

fn write_execution_receipt(
    temp: &Path,
    loaded: &LoadedCheck,
    result_bytes: &[u8],
    task: &mut velnor_actions_contract::TaskReport,
) -> Result<(), OrchestratorError> {
    let result: NativeCheckResult = serde_json::from_slice(result_bytes)
        .map_err(|_| internal("native_check_result_invalid"))?;
    let receipt = crate::check_evidence::gate::native_execution_receipt(
        &loaded.plan,
        &loaded.entry,
        &result,
        result_bytes,
    )?;
    let receipt_bytes =
        canonical_json_bytes(&receipt).map_err(|_| internal("native_check_receipt_encode"))?;
    if receipt_bytes.len() > velnor_actions_contract::config::MAX_CHECK_EXECUTION_RECEIPT_BYTES {
        return Err(internal("native_check_receipt_too_large"));
    }
    let base = temp
        .join("velnor")
        .join(&loaded.plan.run_key)
        .join(&loaded.entry.matrix_key);
    crate::exclusive_write::create_dir_no_symlink(temp, &base)?;
    crate::exclusive_write::write_exclusive(
        &base.join("check-execution.json"),
        &receipt_bytes,
        "check_execution",
    )?;
    task.outputs.clear();
    Ok(())
}
