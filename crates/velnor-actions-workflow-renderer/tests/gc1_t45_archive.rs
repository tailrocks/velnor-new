// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn config_for(layer_hex: &str) -> Vec<u8> {
    format!(
        "{{\"architecture\":\"amd64\",\"os\":\"linux\",\"rootfs\":{{\"type\":\"layers\",\"diff_ids\":[\"sha256:{layer_hex}\"]}}}}"
    )
    .into_bytes()
}

pub(crate) fn manifest_for(config: &[u8], layer: &[u8]) -> Vec<u8> {
    format!(
        "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"config\":{{\"mediaType\":\"application/vnd.oci.image.config.v1+json\",\"digest\":\"sha256:{config_hex}\",\"size\":{config_len}}},\"layers\":[{{\"mediaType\":\"application/vnd.oci.image.layer.v1.tar\",\"digest\":\"sha256:{layer_hex}\",\"size\":{layer_len}}}]}}",
        config_hex = sha256_hex(config),
        config_len = config.len(),
        layer_hex = sha256_hex(layer),
        layer_len = layer.len(),
    )
    .into_bytes()
}

pub(crate) fn index_for(manifest: &[u8]) -> Vec<u8> {
    format!(
        "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"manifests\":[{{\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"digest\":\"sha256:{manifest_hex}\",\"size\":{manifest_len},\"platform\":{{\"architecture\":\"amd64\",\"os\":\"linux\"}}}}]}}",
        manifest_hex = sha256_hex(manifest),
        manifest_len = manifest.len(),
    )
    .into_bytes()
}

pub(crate) fn assemble_archive(
    layer: &[u8],
    config: &[u8],
    manifest: &[u8],
    index: &[u8],
    layout: &[u8],
) -> Vec<u8> {
    let manifest_hex = sha256_hex(manifest);
    let config_hex = sha256_hex(config);
    let layer_hex = sha256_hex(layer);
    tar_archive(&[
        ("oci-layout", layout),
        ("index.json", index),
        (&format!("blobs/sha256/{manifest_hex}"), manifest),
        (&format!("blobs/sha256/{config_hex}"), config),
        (&format!("blobs/sha256/{layer_hex}"), layer),
    ])
}

pub(crate) fn tamper_layer_data(archive: &[u8], parts: &ImageParts) -> Vec<u8> {
    let mut offset = 0;
    for blob in [&parts.layout, &parts.index, &parts.manifest, &parts.config] {
        offset += 512 + blob.len().div_ceil(512) * 512;
    }
    let mut out = archive.to_vec();
    out[offset + 1025] ^= 1;
    out
}

pub(crate) fn splice_before_end(mut archive: Vec<u8>, entry: &[u8]) -> Vec<u8> {
    archive.truncate(archive.len() - 1024);
    archive.extend_from_slice(entry);
    archive.extend_from_slice(&[0u8; 1024]);
    archive
}

pub(crate) fn image_parts() -> ImageParts {
    let layer = tar_archive(&[("hello.txt", b"hello".as_slice())]);
    let config = config_for(&sha256_hex(&layer));
    let manifest = manifest_for(&config, &layer);
    let index = index_for(&manifest);
    let layout = b"{\"imageLayoutVersion\":\"1.0.0\"}".to_vec();
    ImageParts {
        layer,
        config,
        manifest,
        index,
        layout,
    }
}

pub(crate) fn archive_bytes(parts: &ImageParts) -> Vec<u8> {
    assemble_archive(
        &parts.layer,
        &parts.config,
        &parts.manifest,
        &parts.index,
        &parts.layout,
    )
}

pub(crate) fn fixture() -> Result<(Fixture, ImageParts, Vec<u8>), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let driver = stage_fixed_inputs(root.path())?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    Ok((Fixture { root, driver }, parts, archive))
}

pub(crate) fn write_bytes(root: &Path, name: &str, bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let path = root.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path
        .to_str()
        .ok_or_else(|| fail("fixture path left its string shape"))?
        .to_owned())
}

pub(crate) fn record_json(parts: &ImageParts, archive: &[u8]) -> String {
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    format!(
        "{{\"schema\":\"velnor.job-image-candidate.v1\",\"source\":{{\"commit\":\"{}\",\"ref\":\"refs/heads/main\",\"inputs\":{{\".velnor/runner.lock\":\"{}\",\"images/job/ubuntu-26.04/Dockerfile\":\"{}\",\"images/job/ubuntu-26.04/README.md\":\"{}\",\"images/job/ubuntu-26.04/build-test.sh\":\"{}\",\"images/job/ubuntu-26.04/candidate/candidate.py\":\"{}\"}}}}",
        "0".repeat(40),
        "0".repeat(64),
        "1".repeat(64),
        "2".repeat(64),
        "3".repeat(64),
        "4".repeat(64),
    ) + &format!(
        ",\"workflow\":{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1,\"event\":\"push\",\"workflow_ref\":\"tailrocks/velnor-new/.github/workflows/image.yml@refs/heads/main\",\"authority_commit\":\"{}\",\"run_id\":1,\"run_attempt\":1}},\"platform\":\"linux/amd64\",\"build\":{{\"builder\":\"github.com/docker/buildx v0.25.0\",\"platform\":\"linux/amd64\",\"arguments\":[],\"exporter\":\"oci\",\"provenance\":\"disabled\",\"compression\":\"gzip\",\"force_compression\":true}},\"archive\":{{\"sha256\":\"{}\",\"size_bytes\":{}}},\"oci\":{{\"archive_index_digest\":\"{}\",\"archive_index_size_bytes\":{},\"platform_manifest_digest\":\"sha256:{}\",\"platform_manifest_size_bytes\":{},\"image_configuration_digest\":\"sha256:{}\",\"image_configuration_size_bytes\":{},\"layer_digests\":[\"sha256:{}\"]}}}}",
        "1".repeat(40),
        sha256_hex(archive),
        archive.len(),
        sha256_hex(&parts.index),
        parts.index.len(),
        manifest_hex,
        parts.manifest.len(),
        config_hex,
        parts.config.len(),
        layer_hex,
    )
}
