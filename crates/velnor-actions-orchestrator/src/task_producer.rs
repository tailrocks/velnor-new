// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::task_execution::unique_record_by_execution_digest;
use std::{env, path::Path};
use velnor_actions_contract::workflow::{
    TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1, task_execution_manifest_marker_line,
};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json};

pub(crate) fn authorize_producer<'a>(
    plan: &'a velnor_actions_contract::Plan,
    root: &Path,
    job: &str,
    task: &str,
    digest: Option<&str>,
) -> Result<(velnor_actions_contract::MatrixEntry, &'a str), OrchestratorError> {
    validate_producer_context(plan)?;
    let record = read_authorized_producer(root, &plan.head, job, task, digest)?;
    let obligation = plan
        .obligations
        .iter()
        .find(|item| item.task_id == task)
        .ok_or_else(|| internal("producer_obligation_missing"))?;
    if obligation.decision != velnor_actions_contract::ObligationDecision::Execute
        || record.task_id != task
        || record.task_digest != obligation.task_digest
    {
        return Err(internal("producer_obligation_mismatch"));
    }
    let mut entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id == task && entry.stack_id == "rust")
        .cloned()
        .ok_or_else(|| internal("producer_task_not_in_plan"))?;
    if entry.task_digest != obligation.task_digest {
        return Err(internal("producer_obligation_mismatch"));
    }
    let mut tasks = Vec::new();
    for value in entry.execute_task_ids.tasks.values() {
        match value {
            velnor_actions_contract::ExecuteTaskRef::Single(id) => tasks.push(id.as_str()),
            velnor_actions_contract::ExecuteTaskRef::Shards(ids) => {
                tasks.extend(ids.iter().map(String::as_str));
            }
        }
    }
    if tasks.as_slice() != [task] {
        return Err(internal("producer_task_mismatch"));
    }
    entry.id.clone_from(&record.matrix_id);
    entry.matrix_key.clone_from(&record.matrix_key);
    entry.report_id =
        velnor_actions_contract::report_id_for_matrix(&plan.run_key, &entry.matrix_key)
            .map_err(internal_contract)?;
    entry.job_id.clone_from(&obligation.job_id);
    entry.artifact_id =
        velnor_actions_contract::artifact_id_for_crate_job(&plan.run_key, &entry.job_id)
            .map_err(internal_contract)?;
    entry.lane_variant = None;
    entry.validate(&plan.run_key).map_err(internal_contract)?;
    Ok((entry, obligation.task_digest.as_str()))
}

fn validate_producer_context(
    plan: &velnor_actions_contract::Plan,
) -> Result<(), OrchestratorError> {
    if plan.event != velnor_actions_contract::WorkflowEvent::Push
        || env::var("GITHUB_EVENT_NAME").ok().as_deref() != Some("push")
        || env::var("GITHUB_REF_PROTECTED").ok().as_deref() != Some("true")
    {
        return Err(internal("producer_event_not_authorized"));
    }
    if env::var("GITHUB_SHA").ok().as_deref() != Some(&plan.head) {
        return Err(internal("producer_source_mismatch"));
    }
    let payload_path = env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("producer_event_missing"))?;
    let payload = crate::safe_read::read_event_file(
        Path::new(&payload_path),
        crate::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let payload = parse_strict_json(&payload).map_err(|_| internal("producer_event_invalid"))?;
    let branch = payload
        .pointer("/repository/default_branch")
        .and_then(serde_json::Value::as_str)
        .filter(|branch| !branch.is_empty())
        .ok_or_else(|| internal("producer_event_invalid"))?;
    let expected_ref = format!("refs/heads/{branch}");
    if payload.get("ref").and_then(serde_json::Value::as_str) != Some(expected_ref.as_str())
        || env::var("GITHUB_REF").ok().as_deref() != Some(expected_ref.as_str())
        || payload.get("after").and_then(serde_json::Value::as_str) != Some(plan.head.as_str())
    {
        return Err(internal("producer_event_not_authorized"));
    }
    Ok(())
}

fn read_authorized_producer(
    root: &Path,
    head: &str,
    job: &str,
    task: &str,
    digest: Option<&str>,
) -> Result<velnor_actions_contract::TaskExecutionManifestEntryV1, OrchestratorError> {
    let (raw, descriptor_raw) = producer_source_blobs(root, head)?;
    let (marker, body) = raw
        .split_once('\n')
        .ok_or_else(|| internal("malformed_task_execution_marker"))?;
    let body = body
        .strip_suffix('\n')
        .ok_or_else(|| internal("malformed_task_execution_document"))?;
    let value =
        parse_strict_json(body).map_err(|_| internal("unparsable_task_execution_manifest"))?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_value(value)
        .map_err(|_| internal("unparsable_task_execution_manifest"))?;
    manifest.validate().map_err(internal_contract)?;
    if task_execution_manifest_marker_line(&manifest.generator_version)
        .map_err(internal_contract)?
        != marker
        || canonical_json_bytes(&manifest).map_err(internal_contract)? != body.as_bytes()
    {
        return Err(internal("noncanonical_task_execution_manifest"));
    }
    let (descriptor_marker, descriptor_body) = descriptor_raw
        .split_once('\n')
        .ok_or_else(|| internal("unparsable_producer_manifest"))?;
    let descriptor_body = descriptor_body
        .strip_suffix('\n')
        .ok_or_else(|| internal("unparsable_producer_manifest"))?;
    if descriptor_marker != marker {
        return Err(internal("producer_manifest_marker_mismatch"));
    }
    let value =
        parse_strict_json(descriptor_body).map_err(|_| internal("unparsable_producer_manifest"))?;
    let descriptor: velnor_actions_contract::workflow::producer_execution_manifest::ProducerExecutionManifest = serde_json::from_value(value).map_err(|_| internal("unparsable_producer_manifest"))?;
    descriptor.validate(&manifest).map_err(internal_contract)?;
    if canonical_json_bytes(&descriptor).map_err(internal_contract)? != descriptor_body.as_bytes()
        || descriptor.task_manifest_digest != velnor_actions_contract::digest_b3(raw.as_bytes())
    {
        return Err(internal("producer_manifest_binding_mismatch"));
    }
    let authorized = descriptor
        .producer_tasks
        .get(job)
        .and_then(|tasks| tasks.get(task))
        .ok_or_else(|| internal("producer_task_not_in_source"))?;
    if digest.is_some_and(|digest| digest != authorized) {
        return Err(internal("producer_execution_digest_mismatch"));
    }
    let record = unique_record_by_execution_digest(&manifest, authorized)?.clone();
    Ok(record)
}

fn producer_source_blobs(root: &Path, head: &str) -> Result<(String, String), OrchestratorError> {
    use velnor_actions_contract::workflow::producer_execution_manifest::PRODUCER_EXECUTION_MANIFEST_PATH;
    let args = [
        TASK_EXECUTION_MANIFEST_PATH,
        PRODUCER_EXECUTION_MANIFEST_PATH,
    ]
    .into_iter()
    .map(|path| std::ffi::OsString::from(format!("{head}:{path}")))
    .collect::<Vec<_>>();
    crate::validators::validate_select_show_args(&args)
        .map_err(|_| internal("invalid_producer_source"))?;
    let output = velnor_actions_mise::GitRequest::show(args)
        .command_in(root)
        .with_env(&[(
            std::ffi::OsString::from("GIT_NO_REPLACE_OBJECTS"),
            std::ffi::OsString::from("1"),
        )])
        .map_err(|_| internal("producer_source_unavailable"))?
        .run_bounded(
            2 * velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES,
            std::time::Duration::from_secs(60),
        )
        .map_err(|_| internal("producer_source_unavailable"))?;
    if !output.success {
        return Err(internal("producer_source_unavailable"));
    }
    let text = output
        .stdout_text("git")
        .map_err(|_| internal("producer_source_unavailable"))?;
    let mut lines = text.split_inclusive('\n');
    let first = lines
        .next()
        .ok_or_else(|| internal("producer_source_unavailable"))?;
    let second = lines
        .next()
        .ok_or_else(|| internal("producer_source_unavailable"))?;
    let third = lines
        .next()
        .ok_or_else(|| internal("producer_source_unavailable"))?;
    let fourth = lines
        .next()
        .ok_or_else(|| internal("producer_source_unavailable"))?;
    if !fourth.ends_with('\n') || lines.next().is_some() {
        return Err(internal("producer_source_unavailable"));
    }
    let raw = format!("{first}{second}");
    let descriptor = format!("{third}{fourth}");
    let limit = velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
    if raw.len() > limit || descriptor.len() > limit {
        return Err(internal("producer_source_too_large"));
    }
    Ok((raw, descriptor))
}
