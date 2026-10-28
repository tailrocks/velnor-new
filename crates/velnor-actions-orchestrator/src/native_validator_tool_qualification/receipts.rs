// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};

use super::{Archive, Context, MAX_RECEIPT_BYTES, contains_line, read_runner_text, sha256};
use crate::OrchestratorError;
use crate::internal::internal;
use crate::safe_read::{RepoBytes, RepoRead, read_repo_bytes, read_repo_file};

pub(super) fn admitted_archives(
    context: &Context,
    evidence: &Path,
    phase: &Path,
) -> Result<Vec<Archive>, OrchestratorError> {
    let RepoBytes::Bytes(lock_bytes) =
        read_repo_bytes(&context.workspace, "mise.lock", MAX_RECEIPT_BYTES)?
    else {
        return Err(internal("native_validator_lock_unavailable"));
    };
    let lock_sha256 = sha256(&lock_bytes);
    let admission = read_runner_text(
        &evidence.join(super::ADMISSION_FILE),
        "native_validator_lock_admission",
    )?;
    if !contains_line(&admission, &format!("source_head={}", context.source_head))
        || !contains_line(&admission, &format!("lock_sha256={lock_sha256}"))
        || !has_success_exit(&evidence.join("logs/validator-tool-install.exit"))?
        || !has_success_exit(&evidence.join("logs/validator-tool-lock-admission.exit"))?
    {
        return Err(internal("native_validator_lock_admission_mismatch"));
    }
    let plan_text = read_runner_text(
        &evidence.join(super::PLAN_FILE),
        "native_validator_tool_plan",
    )?;
    let plan = velnor_actions_contract::strict_json::parse_strict_json_with_limit(
        &plan_text,
        usize::try_from(MAX_RECEIPT_BYTES).unwrap_or(usize::MAX),
    )
    .map_err(crate::internal::internal_contract)?;
    if string_field(&plan, "lock_sha256")? != lock_sha256
        || string_field(&plan, "platform")? != "linux-x64"
    {
        return Err(internal("native_validator_tool_plan_lock_mismatch"));
    }
    let tools = plan
        .get("tools")
        .and_then(Value::as_array)
        .filter(|tools| tools.len() == 2)
        .ok_or_else(|| internal("native_validator_tool_plan_shape"))?;
    let mut archives = Vec::with_capacity(2);
    for (index, (tool_name, archive_name)) in [
        ("jactionlint", "jactionlint-linux-x64.tar.gz"),
        ("shellcheck", "shellcheck-linux-x64.tar.xz"),
    ]
    .into_iter()
    .enumerate()
    {
        let tool = &tools[index];
        let url = string_field(tool, "url")?;
        let checksum = string_field(tool, "checksum")?;
        let digest = checksum
            .strip_prefix("sha256:")
            .filter(|value| velnor_actions_contract::ids::is_lower_hex_len(value, 64));
        if string_field(tool, "tool")? != tool_name
            || !url.starts_with("https://")
            || url.bytes().any(|byte| byte.is_ascii_control())
            || digest.is_none()
        {
            return Err(internal("native_validator_tool_plan_tool_mismatch"));
        }
        archives.push(Archive {
            url: url.to_owned(),
            checksum: digest.unwrap_or_default().to_owned(),
            path: phase.join("archives").join(archive_name),
        });
    }
    Ok(archives)
}

pub(super) fn write_request(
    context: &Context,
    phase: &Path,
    archives: &[Archive],
) -> Result<(), OrchestratorError> {
    let RepoRead::Text(template) = read_repo_file(
        &context.workspace,
        super::RECIPE_FILE,
        MAX_RECEIPT_BYTES,
    )? else {
        return Err(internal("native_validator_recipe_unavailable"));
    };
    let mut request = velnor_actions_contract::strict_json::parse_strict_json_with_limit(
        &template,
        usize::try_from(MAX_RECEIPT_BYTES).unwrap_or(usize::MAX),
    )
    .map_err(crate::internal::internal_contract)?;
    let tools = request
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .filter(|tools| tools.len() == archives.len())
        .ok_or_else(|| internal("native_validator_recipe_shape"))?;
    for (index, archive) in archives.iter().enumerate() {
        let path = archive
            .path
            .to_str()
            .ok_or_else(|| internal("native_validator_archive_path_encoding"))?;
        let tool = &mut tools[index];
        let artifacts = tool
            .get_mut("recipe")
            .and_then(|recipe| recipe.get_mut("platforms"))
            .and_then(Value::as_array_mut)
            .and_then(|platforms| platforms.first_mut())
            .and_then(|platform| platform.get_mut("artifacts"))
            .and_then(Value::as_array_mut)
            .and_then(|artifacts| artifacts.first_mut())
            .ok_or_else(|| internal("native_validator_recipe_artifact"))?;
        let artifact = artifacts
            .as_object_mut()
            .ok_or_else(|| internal("native_validator_recipe_artifact"))?;
        artifact.insert("url".to_owned(), Value::String(archive.url.clone()));
        tool.as_object_mut()
            .ok_or_else(|| internal("native_validator_recipe_tool"))?
            .insert(
                "archives".to_owned(),
                json!([{
                    "path": path,
                    "url": archive.url.clone(),
                    "sha256": archive.checksum.clone()
                }]),
            );
    }
    let mut bytes = serde_json::to_vec_pretty(&request)
        .map_err(|error| internal(&format!("native_validator_request_encode:{error}")))?;
    bytes.push(b'\n');
    let request_path = phase.join("request.json");
    let mut file = super::create_private_file(&request_path)?;
    file.write_all(&bytes)
        .map_err(|error| super::io_error(&request_path, error))
}

fn has_success_exit(path: &Path) -> Result<bool, OrchestratorError> {
    let text = read_runner_text(path, "native_validator_install_status")?;
    Ok(contains_line(&text, "0"))
}

fn string_field<'a>(value: &'a Value, name: &str) -> Result<&'a str, OrchestratorError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| internal("native_validator_receipt_shape"))
}
