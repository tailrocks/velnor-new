// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) struct ImageParts {
    pub(crate) layer: Vec<u8>,
    pub(crate) config: Vec<u8>,
    pub(crate) manifest: Vec<u8>,
    pub(crate) index: Vec<u8>,
    pub(crate) layout: Vec<u8>,
}

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
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    tar_archive(&[
        ("oci-layout", &parts.layout),
        ("index.json", &parts.index),
        (&format!("blobs/sha256/{manifest_hex}"), &parts.manifest),
        (&format!("blobs/sha256/{config_hex}"), &parts.config),
        (&format!("blobs/sha256/{layer_hex}"), &parts.layer),
    ])
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
        sha256_hex(&parts.manifest),
        parts.manifest.len(),
        sha256_hex(&parts.config),
        parts.config.len(),
        sha256_hex(&parts.layer),
    )
}

pub(crate) fn identity_envelope() -> String {
    format!(
        "{{\"source\":\"{SOURCE_SHA}\",\"workflow_ref\":\"{WORKFLOW_REF}\",\"run_id\":7,\"run_attempt\":2,\"artifact_id\":9}}",
    )
}

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) driver: std::path::PathBuf,
}

pub(crate) fn write_fixture_source(
    root: &Path,
    name: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(root.join(name), source)?;
    Ok(())
}

pub(crate) fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    write_fixture_source(root.path(), IDENTITY_FILE, IDENTITY_SOURCE)?;
    write_fixture_source(root.path(), REGISTRY_FILE, REGISTRY_SOURCE)?;
    write_fixture_source(root.path(), PUBLISH_FILE, PUBLISH_SOURCE)?;
    write_fixture_source(root.path(), RECORD_FILE, RECORD_SOURCE)?;
    write_fixture_source(root.path(), OCI_FILE, OCI_SOURCE)?;
    write_fixture_source(root.path(), TAR_FILE, TAR_SOURCE)?;
    let driver = root.path().join("gc1_t48_driver.py");
    std::fs::write(&driver, DRIVER)?;
    for name in [
        IDENTITY_FILE,
        REGISTRY_FILE,
        PUBLISH_FILE,
        RECORD_FILE,
        OCI_FILE,
        TAR_FILE,
    ] {
        let output = Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg(root.path().join(name))
            .output()?;
        if !output.status.success() {
            return Err(fail(&format!(
                "{name} left compilable:{:?}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    Ok(Fixture { root, driver })
}
