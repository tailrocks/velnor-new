// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::crate_release_qualification_witness::{
    WitnessRead, WitnessRequest, invalid, marker, validate_request, verify_artifact, verify_job,
    verify_run,
};
use std::io::{Cursor, Read};

pub struct MetadataEvidence {
    bytes: Vec<u8>,
    source_sha: String,
    source_tree_sha: String,
    source_archive_sha256: String,
    metadata: velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
}

impl std::fmt::Debug for MetadataEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MetadataEvidence")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl MetadataEvidence {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.source_sha
    }
    #[must_use]
    pub fn source_tree_sha(&self) -> &str {
        &self.source_tree_sha
    }
    #[must_use]
    pub fn source_archive_sha256(&self) -> &str {
        &self.source_archive_sha256
    }
    #[must_use]
    pub fn metadata_config(
        &self,
    ) -> &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
        &self.metadata
    }
}

pub fn acquire_metadata(
    request: &WitnessRequest,
    api: &dyn WitnessRead,
) -> Result<MetadataEvidence, OrchestratorError> {
    validate_request(request)?;
    let source = request.source_sha.as_deref().ok_or_else(invalid)?;
    if request.metadata.is_none() {
        return Err(invalid());
    }
    let prefix = format!("repos/{}", request.repository);
    let run_path = format!("{prefix}/actions/runs/{}/attempts/1", request.run_id);
    let job_path = format!("{prefix}/actions/jobs/{}", request.job_id);
    let run = api.get(&run_path)?;
    verify_run(&run, request)?;
    let job = api.get(&job_path)?;
    verify_job(&job, request)?;
    let steps = job["steps"].as_array().ok_or_else(invalid)?;
    for name in ["Bind metadata artifact", "Bind approved metadata source"] {
        let selected: Vec<_> = steps.iter().filter(|s| s["name"] == name).collect();
        if selected.len() != 1
            || selected[0]["status"] != "completed"
            || selected[0]["conclusion"] != "success"
        {
            return Err(invalid());
        }
    }
    let log = api.witness_job_log(&request.repository, request.job_id)?;
    let (source_tree, archive_sha) = metadata_source(&log, request, source)?;
    let (id, digest) = marker(&log, request, "VELNOR_REHEARSAL_METADATA ")?;
    let artifact_path = format!("{prefix}/actions/artifacts/{id}");
    let artifact = api.get(&artifact_path)?;
    let size = verify_artifact(
        &artifact,
        request,
        id,
        &digest,
        "approved-metadata-observation",
        32 * 1024 * 1024,
    )?;
    let bytes = api.artifact_bytes(&request.repository, id, size, &digest)?;
    if bytes.len() as u64 != size || crate::generator::sha256_hex(&bytes) != digest {
        return Err(invalid());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(invalid());
    }
    let file = zip.by_index(0).map_err(|_| invalid())?;
    if file.name_raw() != b"metadata.json"
        || !file.is_file()
        || file.is_symlink()
        || file.encrypted()
        || file.enclosed_name().is_none()
        || file.size() == 0
        || file.size() > 16 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut output = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut output)
        .map_err(|_| invalid())?;
    if output.len() > 16 * 1024 * 1024
        || api.get(&run_path)? != run
        || api.get(&job_path)? != job
        || api.get(&artifact_path)? != artifact
    {
        return Err(invalid());
    }
    Ok(MetadataEvidence {
        bytes: output,
        source_sha: source.to_owned(),
        source_tree_sha: source_tree,
        source_archive_sha256: archive_sha,
        metadata: request.metadata.clone().ok_or_else(invalid)?,
    })
}

fn metadata_source(
    log: &[u8],
    request: &WitnessRequest,
    source: &str,
) -> Result<(String, String), OrchestratorError> {
    if log.len() > 4 * 1024 * 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(log).map_err(|_| invalid())?;
    if !text.ends_with('\n') {
        return Err(invalid());
    }
    let prefix = "VELNOR_METADATA_SOURCE ";
    let mut found = None;
    for line in text.lines() {
        if line.starts_with(prefix) {
            return Err(invalid());
        }
        if let Some((time, body)) = line.split_once(' ')
            && body.starts_with(prefix)
        {
            if !crate::crate_release_qualification_witness::valid_log_time(time) || found.is_some()
            {
                return Err(invalid());
            }
            let config_path = &request
                .metadata
                .as_ref()
                .ok_or_else(invalid)?
                .intent_config_path;
            let expected = format!(
                "{prefix}run={} attempt=1 source={source} config={config_path} tree=",
                request.run_id
            );
            let tail = body.strip_prefix(&expected).ok_or_else(invalid)?;
            let (tree, digest) = tail.split_once(" sha256=").ok_or_else(invalid)?;
            if tree.len() != 40
                || !tree
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid());
            }
            found = Some((tree.to_owned(), digest.to_owned()));
        }
    }
    found.ok_or_else(invalid)
}

pub fn acquire_output_bundle(
    request: &WitnessRequest,
    api: &dyn WitnessRead,
) -> Result<Vec<u8>, OrchestratorError> {
    validate_request(request)?;
    if !request.output_bundle {
        return Err(invalid());
    }
    let prefix = format!("repos/{}", request.repository);
    let run_path = format!("{prefix}/actions/runs/{}/attempts/1", request.run_id);
    let job_path = format!("{prefix}/actions/jobs/{}", request.job_id);
    let run = api.get(&run_path)?;
    verify_run(&run, request)?;
    let job = api.get(&job_path)?;
    verify_job(&job, request)?;
    let steps = job["steps"].as_array().ok_or_else(invalid)?;
    let binding: Vec<_> = steps
        .iter()
        .filter(|s| s["name"] == "Bind rehearsal output artifact")
        .collect();
    if binding.len() != 1
        || binding[0]["status"] != "completed"
        || binding[0]["conclusion"] != "success"
    {
        return Err(invalid());
    }
    let log = api.witness_job_log(&request.repository, request.job_id)?;
    let (id, digest) = marker(&log, request, "VELNOR_REHEARSAL_OUTPUT ")?;
    let artifact_path = format!("{prefix}/actions/artifacts/{id}");
    let artifact = api.get(&artifact_path)?;
    let size = verify_artifact(
        &artifact,
        request,
        id,
        &digest,
        "rehearsal-output-observation",
        256 * 1024 * 1024,
    )?;
    let bytes = api.artifact_bytes(&request.repository, id, size, &digest)?;
    if bytes.len() as u64 != size || crate::generator::sha256_hex(&bytes) != digest {
        return Err(invalid());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(invalid());
    }
    let file = zip.by_index(0).map_err(|_| invalid())?;
    if file.name_raw() != b"rehearsal-output.tar"
        || !file.is_file()
        || file.is_symlink()
        || file.encrypted()
        || file.enclosed_name().is_none()
        || file.size() == 0
        || file.size() > 100 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut output = Vec::new();
    file.take(100 * 1024 * 1024 + 1)
        .read_to_end(&mut output)
        .map_err(|_| invalid())?;
    if output.len() > 100 * 1024 * 1024
        || api.get(&run_path)? != run
        || api.get(&job_path)? != job
        || api.get(&artifact_path)? != artifact
    {
        return Err(invalid());
    }
    Ok(output)
}
