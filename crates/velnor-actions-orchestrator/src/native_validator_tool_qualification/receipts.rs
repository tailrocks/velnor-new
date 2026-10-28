// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{io::Write, time::Duration, time::Instant};
use std::path::Path;

use serde_json::{Value, json};

use super::{Archive, Context, MAX_RECEIPT_BYTES, contains_line, read_runner_text, sha256};
use crate::OrchestratorError;
use crate::internal::internal;
use crate::safe_read::{RepoBytes, read_repo_bytes};

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
    let recipe_limit = usize::try_from(MAX_RECEIPT_BYTES)
        .map_err(|_| internal("native_validator_recipe_limit"))?;
    let deadline = velnor_actions_mise::CheckDeadline::from_start_with_cleanup_reserve(
        Instant::now(),
        Duration::from_secs(30),
    )
    .map_err(|_| internal("native_validator_recipe_deadline"))?;
    let template = crate::image_maintenance::read_source_file(
        &context.workspace,
        &context.source_head,
        super::RECIPE_FILE,
        recipe_limit,
        deadline,
    )?;
    let template = String::from_utf8(template)
        .map_err(|_| internal("native_validator_recipe_encoding"))?;
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

#[cfg(test)]
mod tests {
    use std::{error::Error, fs, path::Path};

    use serde_json::{Value, json};
    use super::{Archive, Context, write_request};
    use crate::git_fixture;

    const RECIPE: &str =
        include_str!("../../../../scripts/generator-release/source-qualification-validator-recipes.json");

    #[test]
    fn request_uses_recipe_at_selected_source_commit() -> Result<(), Box<dyn Error>> {
        let repository = tempfile::tempdir()?;
        let root = repository.path();
        git(root, &["init", "--quiet"])?;
        let recipe_path = root.join(super::super::RECIPE_FILE);
        fs::create_dir_all(recipe_path.parent().ok_or("recipe parent")?)?;
        fs::write(&recipe_path, RECIPE)?;
        git(root, &["add", "--", super::super::RECIPE_FILE])?;
        git(root, &["commit", "--quiet", "-m", "recipe A"])?;
        let source_head = git(root, &["rev-parse", "HEAD"])?;

        let mut worktree_recipe: Value = serde_json::from_str(RECIPE)?;
        worktree_recipe["timeout_seconds"] = json!(181);
        fs::write(&recipe_path, serde_json::to_vec_pretty(&worktree_recipe)?)?;

        let phase = tempfile::tempdir()?;
        let archives = [
            Archive {
                url: "https://example.invalid/jactionlint.tar.gz".to_owned(),
                checksum: "a".repeat(64),
                path: phase.path().join("archives/jactionlint-linux-x64.tar.gz"),
            },
            Archive {
                url: "https://example.invalid/shellcheck.tar.xz".to_owned(),
                checksum: "b".repeat(64),
                path: phase.path().join("archives/shellcheck-linux-x64.tar.xz"),
            },
        ];
        let context = Context {
            workspace: root.to_path_buf(),
            runner_temp: root.to_path_buf(),
            target_dir: root.to_path_buf(),
            binary_receipt_path: String::new(),
            event_name: "workflow_dispatch".to_owned(),
            source_head,
            current_executable: root.join("velnor-actions"),
            binary_version: "0.1.69".to_owned(),
        };

        write_request(&context, phase.path(), &archives)?;

        let request: Value = serde_json::from_slice(&fs::read(phase.path().join("request.json"))?)?;
        assert_eq!(request["timeout_seconds"], json!(180));
        assert_ne!(request["timeout_seconds"], json!(181));
        let tools = request["tools"].as_array().ok_or("request tools")?;
        for (tool, archive) in tools.iter().zip(&archives) {
            let path = archive.path.to_str().ok_or("archive path")?;
            assert_eq!(
                tool["recipe"]["platforms"][0]["artifacts"][0]["url"],
                archive.url
            );
            assert_eq!(
                tool["archives"],
                json!([{
                    "path": path,
                    "url": archive.url,
                    "sha256": archive.checksum
                }])
            );
        }
        Ok(())
    }

    fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
        let mut command = if root.join(".git").exists() {
            git_fixture::command(root)?
        } else {
            let mut command = git_fixture::sterile_command();
            command.current_dir(root);
            command
        };
        let output = command.args(args).output()?;
        if !output.status.success() {
            return Err(format!(
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
}
