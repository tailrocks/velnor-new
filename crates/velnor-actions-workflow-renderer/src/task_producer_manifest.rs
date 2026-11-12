// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, render::RenderedFile};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::producer_execution_manifest::{
    PRODUCER_EXECUTION_MANIFEST_PATH, ProducerExecutionManifest,
};
use velnor_actions_contract::workflow::{
    TaskExecutionManifestV1, task_execution_manifest_marker_line,
};

pub(crate) fn render(
    manifest: &TaskExecutionManifestV1,
    bytes: &str,
    producer_tasks: BTreeMap<String, BTreeMap<String, String>>,
    generator_version: &str,
) -> Result<Option<RenderedFile>, RenderError> {
    if producer_tasks.is_empty() {
        return Ok(None);
    }
    let descriptor = ProducerExecutionManifest {
        schema: 1,
        task_manifest_digest: velnor_actions_contract::digest_b3(bytes.as_bytes()),
        producer_tasks,
    };
    descriptor
        .validate(manifest)
        .map_err(RenderError::Contract)?;
    let data = velnor_actions_contract::canonical_json_bytes(&descriptor)
        .map_err(RenderError::Contract)?;
    let body =
        String::from_utf8(data).map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let bytes = format!(
        "{}\n{}\n",
        task_execution_manifest_marker_line(generator_version).map_err(RenderError::Contract)?,
        body
    );
    if bytes.len() > velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES {
        return Err(RenderError::InvalidWorkflow(
            "producer_manifest_too_large".to_owned(),
        ));
    }
    Ok(Some(RenderedFile {
        path: PRODUCER_EXECUTION_MANIFEST_PATH.to_owned(),
        bytes,
    }))
}

pub(crate) fn record(
    tasks: &mut BTreeMap<String, BTreeMap<String, String>>,
    job_id: &str,
    job: &velnor_actions_contract::Job,
    entry: &velnor_actions_contract::TaskExecutionManifestEntryV1,
) -> Result<(), RenderError> {
    if job.producer_kind() == Some(velnor_actions_contract::workflow::ProducerKind::StateProducer) {
        let tasks = tasks.entry(job_id.to_owned()).or_default();
        if let Some(previous) = tasks.insert(entry.task_id.clone(), entry.execution_digest.clone())
            && previous != entry.execution_digest
        {
            return Err(RenderError::InvalidWorkflow(
                "producer_task_identity_conflict".to_owned(),
            ));
        }
    }
    Ok(())
}
