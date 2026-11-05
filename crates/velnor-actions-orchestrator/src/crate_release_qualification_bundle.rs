// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::crate_release_qualification_witness::{
    WitnessRead, WitnessRequest, invalid, marker, validate_request, verify_artifact, verify_job,
    verify_run,
};
use std::io::{Cursor, Read};

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
