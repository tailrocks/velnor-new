// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    MAX_CHECK_EXECUTION_RECEIPT_BYTES, MatrixEntry, Path, declaration_for, parse_strict_json,
    reject_link_components,
};

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
        velnor_actions_contract::staged_reads::read_staged_text(
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
        Some(velnor_actions_contract::staged_reads::read_staged_text(
            &dir.join(path),
            crate::safe_read::MAX_REPO_FILE_BYTES,
        )?)
    } else {
        None
    };
    Ok(serde_json::json!({"execution": execution, "evidence": evidence}))
}
