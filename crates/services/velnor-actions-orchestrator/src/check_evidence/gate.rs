//! Mandatory named-check execution and scenario proof at the ordinary final fold.
use super::{EvidenceReceipt, ScenarioEvidence, reject_link_components, validate_evidence};
use serde::{Deserialize, Serialize};
pub(crate) mod container;
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
        schema: 1,
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
    }
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
        if entry.stack_id != "mise" {
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
    let declaration = declaration_for(entry)?;
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
) -> bool {
    if plan.obligations.iter().any(|ob| {
        ob.task_id.starts_with("stack/mise/")
            && ob.decision != velnor_actions_contract::ObligationDecision::Execute
    }) {
        return false;
    }
    if tasks.iter().any(|task| {
        task.task_id.starts_with("stack/mise/")
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
            entry.stack_id == "mise" && entry.matrix_key == proof.execution.matrix_key
        }) else {
            return false;
        };
        if !seen.insert(entry.matrix_key.as_str()) || !valid_proof(plan, entry, &proof) {
            return false;
        }
    }
    for entry in &plan.matrix.include {
        if entry.stack_id != "mise" {
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
            let Ok(declaration) = declaration_for(entry) else {
                return false;
            };
            let expected = declaration.map_or_else(Vec::new, |evidence| vec![evidence.path]);
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

fn valid_proof(plan: &Plan, entry: &MatrixEntry, proof: &DownloadedProof) -> bool {
    let receipt = &proof.execution;
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
    if receipt.schema != 1
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

fn valid_native_proofs(
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

fn valid_qualified_proofs(
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
