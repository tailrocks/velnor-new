//! Mandatory named-check execution and scenario proof at the ordinary final fold.
use super::{EvidenceReceipt, ScenarioEvidence, reject_link_components, validate_evidence};
use serde::{Deserialize, Serialize};
#[path = "check_container_gate.rs"]
pub(crate) mod container;
#[path = "check_tool_proof.rs"]
pub(crate) mod tools;

use std::collections::BTreeSet;
use std::path::Path;
use velnor_actions_contract::config::MAX_CHECK_EXECUTION_RECEIPT_BYTES;
use velnor_actions_contract::config::{CheckEvidence, CheckPlatform, CheckRunner};
use velnor_actions_contract::{
    MatrixEntry, Plan, TaskReport, TaskStatus, digest_b3, parse_strict_json,
};

/// Helper-produced execution receipt inside the existing matrix artifact.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckExecutionReceipt {
    pub schema: u32,
    pub executor: CheckExecutionExecutor,
    pub source: String,
    pub run_key: String,
    pub head: String,
    pub check_id: String,
    pub task_id: String,
    pub task_digest: String,
    pub input_digest: String,
    pub matrix_key: String,
    pub platform: CheckPlatform,
    pub evidence: Option<EvidenceReceipt>,
    pub container: Option<container::ContainerReceipt>,
    pub system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    pub qualified_tools: Vec<tools::QualifiedToolReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeCheckExecutionReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckExecutionExecutor {
    Mise,
    NativeHomebrew,
}

/// Typed proof emitted by the SDK-native Homebrew checker.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeCheckExecutionReceipt {
    pub schema: u32,
    pub operation: velnor_actions_contract::config::NativeHomebrewOperation,
    pub identity: crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    pub result_digest: String,
    pub result: serde_json::Value,
}

/// Downloaded bytes travel with the receipt; merge never trusts assembly alone.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DownloadedProof {
    execution: CheckExecutionReceipt,
    evidence: Option<String>,
}

pub(crate) fn execution_receipt(
    plan: &Plan,
    entry: &MatrixEntry,
    check_id: &str,
    platform: CheckPlatform,
    evidence: Option<EvidenceReceipt>,
    system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    qualified_tools: Vec<tools::QualifiedToolReceipt>,
) -> CheckExecutionReceipt {
    CheckExecutionReceipt {
        schema: 2,
        executor: CheckExecutionExecutor::Mise,
        source: "execute-check-v1".into(),
        run_key: plan.run_key.clone(),
        head: plan.head.clone(),
        check_id: check_id.into(),
        task_id: entry.task_id.clone(),
        task_digest: entry.task_digest.clone(),
        input_digest: entry.input_digest.clone(),
        matrix_key: entry.matrix_key.clone(),
        platform,
        evidence,
        container: None,
        system_tools,
        qualified_tools,
        native: None,
    }
}

pub(crate) fn native_execution_receipt(
    plan: &Plan,
    entry: &MatrixEntry,
    result: &crate::homebrew::runtime::NativeCheckResult,
    result_bytes: &[u8],
) -> Result<CheckExecutionReceipt, crate::OrchestratorError> {
    let identity = result.identity.clone();
    let result_value = serde_json::from_slice(result_bytes)
        .map_err(|_| crate::internal::internal("native_check_result_invalid"))?;
    if velnor_actions_contract::canonical_json_bytes(&result)
        .map_err(crate::internal::internal_contract)?
        != result_bytes
    {
        return Err(crate::internal::internal(
            "native_check_result_noncanonical",
        ));
    }
    Ok(CheckExecutionReceipt {
        schema: 2,
        executor: CheckExecutionExecutor::NativeHomebrew,
        source: crate::internal_plan::native_homebrew_checks::EXECUTE_OPERATION.to_owned(),
        run_key: plan.run_key.clone(),
        head: plan.head.clone(),
        check_id: identity.check_id.clone(),
        task_id: entry.task_id.clone(),
        task_digest: entry.task_digest.clone(),
        input_digest: entry.input_digest.clone(),
        matrix_key: entry.matrix_key.clone(),
        platform: identity.platform,
        evidence: None,
        container: None,
        system_tools: Vec::new(),
        qualified_tools: Vec::new(),
        native: Some(NativeCheckExecutionReceipt {
            schema: 1,
            operation: identity.operation,
            identity,
            result_digest: digest_b3(result_bytes),
            result: result_value,
        }),
    })
}

/// Read only plan-named receipts and evidence from exact downloaded matrix homes.
pub(crate) fn read_proofs(
    plan: &serde_json::Value,
    reports_dir: &Path,
    tasks: &[serde_json::Value],
    errors: &mut Vec<String>,
) -> Vec<serde_json::Value> {
    let Some(entries) = plan
        .pointer("/matrix/include")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    let mut proofs = Vec::new();
    for value in entries {
        let Ok(entry) = serde_json::from_value::<MatrixEntry>(value.clone()) else {
            continue;
        };
        if !matches!(entry.stack_id.as_str(), "mise" | "native") {
            continue;
        }
        if !tasks.iter().any(|task| {
            task.get("task_id").and_then(serde_json::Value::as_str) == Some(entry.task_id.as_str())
                && task.get("status").and_then(serde_json::Value::as_str) == Some("executed")
        }) {
            continue;
        }
        match read_proof(&entry, reports_dir) {
            Ok(proof) => proofs.push(proof),
            Err(error) => errors.push(format!("{error}:{}", entry.matrix_key)),
        }
    }
    proofs
}

fn read_proof(entry: &MatrixEntry, dir: &Path) -> Result<serde_json::Value, &'static str> {
    velnor_actions_contract::validate_artifact_id(&entry.artifact_id)
        .map_err(|_| "bad_check_artifact")?;
    velnor_actions_contract::validate_matrix_key(&entry.matrix_key)
        .map_err(|_| "bad_check_matrix")?;
    let direct = format!(
        "{}/{}/check-execution.json",
        entry.artifact_id, entry.matrix_key
    );
    let nested = format!(
        "{}/{}/{}/check-execution.json",
        entry.artifact_id, entry.artifact_id, entry.matrix_key
    );
    let read = |relative: &str| {
        reject_link_components(dir, relative).map_err(|_| "symlink_check_execution")?;
        crate::retrieve_reports::read_staged_text(
            &dir.join(relative),
            MAX_CHECK_EXECUTION_RECEIPT_BYTES as u64,
        )
    };
    let (relative, text) = match read(&direct) {
        Ok(text) => (direct, text),
        Err("missing") => (nested.clone(), read(&nested)?),
        Err(error) => return Err(error),
    };
    let execution = parse_strict_json(&text).map_err(|_| "unparsable_check_execution")?;
    let declaration = if entry.stack_id == "mise" {
        declaration_for(entry)?
    } else {
        None
    };
    let evidence = if let Some(declaration) = declaration {
        let home = Path::new(&relative).parent().ok_or("bad_check_path")?;
        let path = home.join("evidence").join(&declaration.path);
        let path = path.to_str().ok_or("bad_check_path")?;
        reject_link_components(dir, path).map_err(|_| "symlink_check_evidence")?;
        Some(crate::retrieve_reports::read_staged_text(
            &dir.join(path),
            crate::safe_read::MAX_REPO_FILE_BYTES,
        )?)
    } else {
        None
    };
    Ok(serde_json::json!({"execution": execution, "evidence": evidence}))
}

fn declaration_for(entry: &MatrixEntry) -> Result<Option<CheckEvidence>, &'static str> {
    serde_json::from_value(
        entry
            .adapter_metadata
            .get("evidence")
            .cloned()
            .ok_or("missing_check_declaration")?,
    )
    .map_err(|_| "invalid_check_declaration")
}

/// Every successful named task needs exactly one valid proof; foreign proofs fail.
pub(crate) fn validate_proofs(
    plan: &Plan,
    tasks: &[TaskReport],
    proofs: &[serde_json::Value],
    native_root: Option<&Path>,
) -> bool {
    if plan.obligations.iter().any(|ob| {
        (ob.task_id.starts_with("stack/mise/") || ob.task_id.starts_with("internal/native-check/"))
            && ob.decision != velnor_actions_contract::ObligationDecision::Execute
    }) {
        return false;
    }
    if tasks.iter().any(|task| {
        (task.task_id.starts_with("stack/mise/")
            || task.task_id.starts_with("internal/native-check/"))
            && matches!(task.status, TaskStatus::Reused | TaskStatus::EmptyPartition)
    }) {
        return false;
    }
    let mut seen = BTreeSet::new();
    for value in proofs {
        let Ok(proof) = serde_json::from_value::<DownloadedProof>(value.clone()) else {
            return false;
        };
        let Some(entry) = plan.matrix.include.iter().find(|entry| {
            matches!(entry.stack_id.as_str(), "mise" | "native")
                && entry.matrix_key == proof.execution.matrix_key
        }) else {
            return false;
        };
        if !seen.insert(entry.matrix_key.as_str()) || !valid_proof(plan, entry, &proof, native_root)
        {
            return false;
        }
    }
    for entry in &plan.matrix.include {
        if !matches!(entry.stack_id.as_str(), "mise" | "native") {
            continue;
        }
        let successful = tasks.iter().any(|task| {
            task.task_id == entry.task_id
                && task.status == TaskStatus::Executed
                && task.exit_code == 0
        });
        if successful && !seen.contains(entry.matrix_key.as_str()) {
            return false;
        }
        if successful {
            let expected = if entry.stack_id == "native" {
                Vec::new()
            } else {
                let Ok(declaration) = declaration_for(entry) else {
                    return false;
                };
                declaration.map_or_else(Vec::new, |evidence| vec![evidence.path])
            };
            if tasks
                .iter()
                .any(|task| task.task_id == entry.task_id && task.outputs != expected)
            {
                return false;
            }
        }
    }
    true
}

fn valid_proof(
    plan: &Plan,
    entry: &MatrixEntry,
    proof: &DownloadedProof,
    native_root: Option<&Path>,
) -> bool {
    let receipt = &proof.execution;
    if entry.stack_id == "native" {
        return valid_native_check_proof(plan, entry, proof, native_root);
    }
    let Some(check_id) = entry
        .adapter_metadata
        .get("check_id")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let Ok(runner) = serde_json::from_value::<CheckRunner>(
        entry
            .adapter_metadata
            .get("runner")
            .cloned()
            .unwrap_or_default(),
    ) else {
        return false;
    };
    if receipt.schema != 2
        || receipt.executor != CheckExecutionExecutor::Mise
        || receipt.source != "execute-check-v1"
        || receipt.run_key != plan.run_key
        || receipt.head != plan.head
        || receipt.check_id != check_id
        || receipt.task_id != entry.task_id
        || receipt.task_digest != entry.task_digest
        || receipt.input_digest != entry.input_digest
        || receipt.matrix_key != entry.matrix_key
        || receipt.platform != runner.platform
    {
        return false;
    }
    if container::validate_container_receipt(&runner, receipt.container.as_ref()).is_err()
        || !valid_qualified_proofs(entry, runner.platform, &receipt.qualified_tools)
        || !valid_native_proofs(entry, runner.platform, &receipt.system_tools)
    {
        return false;
    }
    let Ok(declaration) = declaration_for(entry) else {
        return false;
    };
    match (declaration, &receipt.evidence, &proof.evidence) {
        (None, None, None) => true,
        (Some(declaration), Some(evidence), Some(bytes)) => {
            if evidence.schema != 1
                || evidence.source != "mise-task-v1"
                || evidence.check_id != check_id
                || evidence.head != plan.head
                || evidence.platform != runner.platform
                || evidence.path != declaration.path
                || evidence.scenarios != declaration.expected_scenarios
                || evidence.digest != digest_b3(bytes.as_bytes())
            {
                return false;
            }
            let Ok(value) = parse_strict_json(bytes) else {
                return false;
            };
            let Ok(producer) = serde_json::from_value::<ScenarioEvidence>(value) else {
                return false;
            };
            validate_evidence(
                &producer,
                &declaration,
                check_id,
                &plan.head,
                runner.platform,
            )
            .is_ok()
        }
        _ => false,
    }
}

#[path = "check_gate_native.rs"]
mod native;

use native::{valid_native_check_proof, valid_native_proofs, valid_qualified_proofs};
