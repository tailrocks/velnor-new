// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn repository_envelope() -> String {
    format!(
        concat!(
            "{{\"repository\":{{\"full_name\":\"tailrocks/velnor-new\",\"id\":1390620900}},",
            "\"tip\":{{\"sha\":\"{source}\"}},\"identity\":{identity}}}"
        ),
        source = SOURCE_SHA,
        identity = identity_envelope(),
    )
}

pub(crate) fn record_source_envelope() -> String {
    format!(
        concat!(
            "{{\"commit\":\"{source}\",\"ref\":\"refs/heads/main\",",
            "\"inputs\":{{\".velnor/runner.lock\":\"{lock}\"}}}}"
        ),
        source = SOURCE_SHA,
        lock = "b".repeat(64),
    )
}

pub(crate) fn record_workflow_envelope() -> String {
    format!(
        concat!(
            "{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1390620900,",
            "\"event\":\"workflow_dispatch\",\"workflow_ref\":\"{workflow_ref}\",",
            "\"authority_commit\":\"{source}\",\"run_id\":7,\"run_attempt\":2}}"
        ),
        workflow_ref = WORKFLOW_REF,
        source = SOURCE_SHA,
    )
}

pub(crate) fn candidate_identity_envelope() -> String {
    format!(
        "{{\"record_source\":{source},\"record_workflow\":{workflow},\"identity\":{identity}}}",
        source = record_source_envelope(),
        workflow = record_workflow_envelope(),
        identity = identity_envelope(),
    )
}

pub(crate) fn artifact_envelope() -> String {
    format!(
        concat!(
            "{{\"response\":{{\"artifacts\":[{{\"name\":\"velnor-job-image-candidate-2\",",
            "\"id\":9,\"expired\":false,\"workflow_run\":{{\"id\":7,",
            "\"head_sha\":\"{source}\",\"head_branch\":\"main\"}}}}]}},",
            "\"identity\":{identity}}}"
        ),
        source = SOURCE_SHA,
        identity = identity_envelope(),
    )
}

pub(crate) fn ci_run(id: u64, number: u64, attempt: u64, status: &str, conclusion: &str) -> String {
    format!(
        concat!(
            "{{\"id\":{id},\"path\":\".github/workflows/ci.yml\",\"event\":\"push\",",
            "\"head_branch\":\"main\",\"head_sha\":\"{source}\",",
            "\"head_repository\":{{\"full_name\":\"tailrocks/velnor-new\"}},",
            "\"run_number\":{number},\"run_attempt\":{attempt},",
            "\"status\":\"{status}\",\"conclusion\":\"{conclusion}\"}}"
        ),
        id = id,
        number = number,
        attempt = attempt,
        status = status,
        conclusion = conclusion,
        source = SOURCE_SHA,
    )
}

pub(crate) fn ci_envelope() -> String {
    format!(
        "{{\"listing\":{{\"workflow_runs\":[{run}]}},\"source\":\"{source}\"}}",
        run = ci_run(11, 5, 1, "completed", "success"),
        source = SOURCE_SHA,
    )
}

pub(crate) fn attestation_envelope() -> String {
    format!(
        concat!(
            "{{\"output\":[{{\"verificationResult\":{{\"signature\":{{\"certificate\":{{",
            "\"sourceRepositoryIdentifier\":1390620900,",
            "\"githubWorkflowRepository\":\"tailrocks/velnor-new\",",
            "\"githubWorkflowSHA\":\"{source}\",",
            "\"githubWorkflowRef\":\"refs/heads/main\",",
            "\"githubWorkflowTrigger\":\"workflow_dispatch\",",
            "\"sourceRepositoryRef\":\"refs/heads/main\",",
            "\"sourceRepositoryDigest\":\"{source}\"",
            "}}}}}}}}],\"source\":\"{source}\"}}"
        ),
        source = SOURCE_SHA,
    )
}

pub(crate) fn source_inputs_envelope() -> String {
    format!(
        "{{\"record_source\":{record},\"authenticated\":{record}}}",
        record = record_source_envelope(),
    )
}

pub(crate) fn build_lock_envelope() -> String {
    "{\"builder\":\"github.com/docker/buildx v0.25.0\",\"buildx_version\":\"0.25.0\"}".to_owned()
}

pub(crate) fn blob_case() -> (Vec<u8>, String, Vec<u8>, String) {
    let config = br#"{"architecture":"amd64","os":"linux"}"#.to_vec();
    let data = b"publication-blob-bytes".to_vec();
    let config_hex = sha256_hex(&config);
    let data_hex = sha256_hex(&data);
    (config, config_hex, data, data_hex)
}

pub(crate) fn manifest_case() -> (Vec<u8>, String) {
    let (config, config_hex, data, data_hex) = blob_case();
    let manifest = format!(
        concat!(
            "{{\"schemaVersion\":2,",
            "\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",",
            "\"config\":{{\"mediaType\":\"application/vnd.oci.image.config.v1+json\",",
            "\"digest\":\"sha256:{config_hex}\",\"size\":{config_len}}},",
            "\"layers\":[{{\"mediaType\":\"application/vnd.oci.image.layer.v1.tar\",",
            "\"digest\":\"sha256:{data_hex}\",\"size\":{data_len}}}]}}"
        ),
        config_hex = config_hex,
        config_len = config.len(),
        data_hex = data_hex,
        data_len = data.len(),
    )
    .into_bytes();
    let manifest_hex = sha256_hex(&manifest);
    (manifest, manifest_hex)
}

pub(crate) fn archive_case() -> (Vec<u8>, Vec<u8>, String, Vec<u8>, String) {
    let (config, config_hex, data, data_hex) = blob_case();
    let (manifest, manifest_hex) = manifest_case();
    let archive = tar_archive(&[
        (
            format!("blobs/sha256/{manifest_hex}").as_str(),
            manifest.as_slice(),
        ),
        (
            format!("blobs/sha256/{config_hex}").as_str(),
            config.as_slice(),
        ),
        (format!("blobs/sha256/{data_hex}").as_str(), data.as_slice()),
    ]);
    (archive, manifest, manifest_hex, data, data_hex)
}

pub(crate) fn write_archive(
    fixture: &Fixture,
    name: &str,
    archive: &[u8],
) -> Result<String, Box<dyn Error>> {
    let path = fixture.root.path().join(name);
    std::fs::write(&path, archive)?;
    Ok(path
        .to_str()
        .ok_or_else(|| fail("archive path left its string shape"))?
        .to_owned())
}
