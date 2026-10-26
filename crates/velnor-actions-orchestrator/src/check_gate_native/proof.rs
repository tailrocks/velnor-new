// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::super::{
    CheckExecutionExecutor, CheckPlatform, DownloadedProof, MatrixEntry, Plan, digest_b3, tools,
};
use crate::homebrew::runtime::{NativeCheckOutcome, NativeCheckResult};
use crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension;

pub(crate) fn valid_native_check_proof(
    plan: &Plan,
    entry: &MatrixEntry,
    proof: &DownloadedProof,
    native_root: Option<&Path>,
) -> bool {
    let receipt = &proof.execution;
    let Some(expected) = deserialize_identity(entry) else {
        return false;
    };
    let Some(native) = &receipt.native else {
        return false;
    };
    let Ok(result) = serde_json::from_value::<NativeCheckResult>(native.result.clone()) else {
        return false;
    };
    let Ok(result_bytes) = velnor_actions_contract::canonical_json_bytes(&result) else {
        return false;
    };
    receipt.schema == 2
        && receipt.executor == CheckExecutionExecutor::NativeHomebrew
        && receipt.source == crate::internal_plan::native_homebrew_checks::EXECUTE_OPERATION
        && receipt.run_key == plan.run_key
        && receipt.head == plan.head
        && receipt.check_id == expected.check_id
        && receipt.task_id == entry.task_id
        && receipt.task_digest == entry.task_digest
        && receipt.input_digest == entry.input_digest
        && receipt.matrix_key == entry.matrix_key
        && receipt.platform == expected.platform
        && native.schema == 1
        && native.operation == expected.operation
        && native.identity == expected
        && result.schema == 1
        && result.identity == expected
        && result.operation == expected.operation
        && native.result_digest == digest_b3(&result_bytes)
        && valid_outcome(plan, &expected, &result, native_root)
        && proof.evidence.is_none()
}

fn deserialize_identity(entry: &MatrixEntry) -> Option<NativeCheckIdentityExtension> {
    let expected = entry.adapter_metadata.get("identity")?;
    serde_json::from_value(expected.clone()).ok()
}

fn valid_outcome(
    plan: &Plan,
    expected: &NativeCheckIdentityExtension,
    result: &NativeCheckResult,
    native_root: Option<&Path>,
) -> bool {
    match &result.outcome {
        NativeCheckOutcome::TapTree { tree_digest } => {
            validate_tap_tree(expected, result.operation, tree_digest, native_root)
        }
        NativeCheckOutcome::FormulaContract {
            formula_bytes,
            public_release_receipt_digest,
        } => validate_formula(
            plan,
            expected,
            result.operation,
            formula_bytes,
            public_release_receipt_digest.as_deref(),
            native_root,
        ),
        NativeCheckOutcome::Install {
            evidence,
            public_release_receipt_digest,
            hosted_profile,
        } => super::install::validate_install(
            plan,
            expected,
            result.operation,
            evidence,
            public_release_receipt_digest.as_str(),
            hosted_profile,
            native_root,
        ),
        NativeCheckOutcome::AdoptionContract { evidence } => {
            validate_adoption(plan, expected, result.operation, evidence, native_root)
        }
    }
}

fn validate_tap_tree(
    expected: &NativeCheckIdentityExtension,
    operation: velnor_actions_contract::config::NativeHomebrewOperation,
    tree_digest: &str,
    native_root: Option<&Path>,
) -> bool {
    let Some(root) = native_root else {
        return false;
    };
    let Some(deadline) = proof_deadline() else {
        return false;
    };
    let Ok(actual) = crate::internal_plan::native_homebrew_checks::read_checked_tap_tree(
        root, expected, deadline,
    ) else {
        return false;
    };
    let actual_bytes = actual
        .iter()
        .map(|item| (&item.path, item.mode, &item.blob_sha))
        .collect::<Vec<_>>();
    let Ok(actual_bytes) = velnor_actions_contract::canonical_json_bytes(&actual_bytes) else {
        return false;
    };
    operation == velnor_actions_contract::config::NativeHomebrewOperation::TapTree
        && tree_digest == digest_b3(&actual_bytes)
}

fn validate_formula(
    plan: &Plan,
    expected: &NativeCheckIdentityExtension,
    operation: velnor_actions_contract::config::NativeHomebrewOperation,
    formula_bytes: &[u8],
    receipt_digest: Option<&str>,
    native_root: Option<&Path>,
) -> bool {
    let Some(root) = native_root else {
        return false;
    };
    let Some(channel) = expected.formula_channel else {
        return false;
    };
    let Some(deadline) = proof_deadline() else {
        return false;
    };
    let Ok((expected_receipt, expected_formula)) =
        crate::homebrew::runtime::verify_formula(root, channel, expected, plan.event, deadline)
    else {
        return false;
    };
    operation == velnor_actions_contract::config::NativeHomebrewOperation::FormulaContract
        && formula_bytes == expected_formula
        && receipt_digest == expected_receipt.as_deref()
}

fn validate_adoption(
    plan: &Plan,
    expected: &NativeCheckIdentityExtension,
    operation: velnor_actions_contract::config::NativeHomebrewOperation,
    evidence: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence,
    native_root: Option<&Path>,
) -> bool {
    let Some(root) = native_root else {
        return false;
    };
    let Some(deadline) = proof_deadline() else {
        return false;
    };
    let Ok(pull_request) =
        crate::internal_plan::native_homebrew_checks::pull_request_number_from_env(plan.event)
    else {
        return false;
    };
    let Ok(seed) = crate::internal_plan::native_homebrew_checks::validate_seed_transition(
        root,
        &expected.tap_repository,
        plan.event,
        pull_request,
        &expected.head,
        deadline,
    ) else {
        return false;
    };
    let transition = seed.transition.transition();
    let Ok(transition_digest) = transition.digest() else {
        return false;
    };
    let expected_authority =
        crate::internal_plan::native_homebrew_checks::NativeCheckAuthorityIdentity::Seed {
            transition_id: transition.transition_id.clone(),
            transition_digest,
            base_tree: transition.base_tree.clone(),
            sdk_source_sha: seed.sdk.source_sha.clone(),
            sdk_version: seed.sdk.version.clone(),
            sdk_manifest_sha256: seed.sdk.manifest_sha256.clone(),
            sdk_seed_runner_binary_sha256: seed.sdk.seed_runner_binary_sha256.clone(),
        };
    operation == velnor_actions_contract::config::NativeHomebrewOperation::AdoptionContract
        && expected.authority == expected_authority
        && evidence == &seed.evidence
}

pub(super) fn proof_deadline() -> Option<velnor_actions_mise::CheckDeadline> {
    velnor_actions_mise::CheckDeadline::from_start(
        std::time::Instant::now(),
        std::time::Duration::from_mins(5),
    )
    .ok()
}

pub(crate) fn valid_native_proofs(
    entry: &MatrixEntry,
    platform: CheckPlatform,
    proofs: &[velnor_actions_mise::checks::SystemToolProof],
) -> bool {
    let Ok(pins) = serde_json::from_value::<Vec<velnor_actions_contract::config::CheckSystemTool>>(
        entry
            .adapter_metadata
            .get("system_tools")
            .cloned()
            .unwrap_or_default(),
    ) else {
        return false;
    };
    velnor_actions_mise::checks::validate_system_tool_proofs(platform, &pins, proofs).is_ok()
}

pub(crate) fn valid_qualified_proofs(
    entry: &MatrixEntry,
    platform: CheckPlatform,
    receipts: &[tools::QualifiedToolReceipt],
) -> bool {
    let Ok(declared) = serde_json::from_value::<Vec<velnor_actions_contract::config::QualifiedTool>>(
        entry
            .adapter_metadata
            .get("qualified_tools")
            .cloned()
            .unwrap_or_default(),
    ) else {
        return false;
    };
    let Ok(specs) = serde_json::from_value::<Vec<String>>(
        entry
            .adapter_metadata
            .get("tool_specs")
            .cloned()
            .unwrap_or_default(),
    ) else {
        return false;
    };
    let Ok(fingerprint) =
        velnor_actions_mise::checks::DiscoveredCheck::qualification_fingerprint(&declared, &specs)
    else {
        return false;
    };
    entry
        .adapter_metadata
        .get("qualification_digest")
        .and_then(serde_json::Value::as_str)
        == Some(fingerprint.as_str())
        && tools::validate_receipts(platform, &declared, receipts)
}
