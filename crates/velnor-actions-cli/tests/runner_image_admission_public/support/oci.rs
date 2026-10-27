// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::ImageSet;

const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
static ARCHIVE_NUMBER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub(crate) struct OciImage {
    pub(crate) path: PathBuf,
    pub(crate) manifest_digest: String,
    pub(crate) config_digest: String,
}

pub(super) fn admission_images(directory: &Path, source: &str) -> Result<ImageSet, Box<dyn Error>> {
    Ok(ImageSet {
        runner: write_image(
            directory,
            "runner",
            &runner_config(source),
            "linux",
            "amd64",
            false,
        )?,
        dind: write_image(
            directory,
            "dind",
            &dind_config(source),
            "linux",
            "amd64",
            false,
        )?,
        resource_probe: write_image(
            directory,
            "probe",
            &probe_config(source),
            "linux",
            "amd64",
            false,
        )?,
    })
}

pub(crate) fn for_kind(
    directory: &Path,
    kind: &str,
    source: &str,
    revision: Option<&str>,
    architecture: Option<&str>,
    corrupt_graph: bool,
) -> Result<OciImage, Box<dyn Error>> {
    let mut config = match kind {
        "runner" => runner_config(source),
        "dind" => dind_config(source),
        "resource_probe" => probe_config(source),
        _ => return Err(format!("unknown image kind: {kind}").into()),
    };
    if let Some(revision) = revision {
        config["Labels"]["org.opencontainers.image.revision"] = json!(revision);
    }
    let filename = next_filename(kind);
    write_image(
        directory,
        &filename,
        &config,
        "linux",
        architecture.unwrap_or("amd64"),
        corrupt_graph,
    )
}

pub(crate) fn probe_mutation(
    directory: &Path,
    source: &str,
    mutation: &str,
) -> Result<OciImage, Box<dyn Error>> {
    let mut config = probe_config(source);
    match mutation {
        "user" => config["User"] = json!("0:0"),
        "working_directory" => config["WorkingDir"] = json!("/tmp"),
        "entrypoint" => config["Entrypoint"] = json!(["/wrong"]),
        "cmd" => config["Cmd"] = json!([]),
        "cmd_null" => config["Cmd"] = Value::Null,
        "env" => config["Env"] = json!(["PATH=/wrong"]),
        "label" => config["Labels"]["release"] = json!("candidate"),
        "key" => config["StopSignal"] = json!("SIGTERM"),
        _ => return Err(format!("unknown probe mutation: {mutation}").into()),
    }
    write_image(
        directory,
        &next_filename("probe-variant"),
        &config,
        "linux",
        "amd64",
        false,
    )
}

fn runner_config(source: &str) -> Value {
    json!({
        "Labels": {"org.opencontainers.image.revision": source},
        "User": "runner-flexible-user",
        "WorkingDir": "/runner-flexible",
        "Entrypoint": ["/runner-flexible"],
        "Cmd": ["--fixture"],
        "Env": ["RUNNER_FIXTURE=1"],
    })
}

fn dind_config(source: &str) -> Value {
    json!({
        "Labels": {"org.opencontainers.image.revision": source},
        "User": "dind-flexible-user",
        "WorkingDir": "/dind-flexible",
        "Entrypoint": ["/dind-flexible"],
        "Cmd": ["--fixture"],
        "Env": ["DIND_FIXTURE=1"],
    })
}

fn probe_config(source: &str) -> Value {
    json!({
        "Labels": {"org.opencontainers.image.revision": source},
        "User": "65532:65532",
        "WorkingDir": "/",
        "Entrypoint": ["/velnor/resource-probe"],
        "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    })
}

fn write_image(
    directory: &Path,
    name: &str,
    config_fields: &Value,
    os: &str,
    architecture: &str,
    corrupt_config: bool,
) -> Result<OciImage, Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    let layer = vec![0_u8; 1024];
    let layer_digest = digest(&layer);
    let config = serde_json::to_vec(&json!({
        "architecture": "amd64",
        "os": "linux",
        "config": config_fields,
        "rootfs": {"type": "layers", "diff_ids": [format!("sha256:{layer_digest}")]},
    }))?;
    let config_digest = digest(&config);
    let stored_config = if corrupt_config {
        let mut changed = config.clone();
        let byte = changed
            .get_mut(0)
            .ok_or("configuration fixture unexpectedly empty")?;
        *byte ^= 1;
        changed
    } else {
        config.clone()
    };
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": descriptor(CONFIG_MEDIA_TYPE, &config_digest, config.len()),
        "layers": [descriptor(LAYER_MEDIA_TYPE, &layer_digest, layer.len())],
    }))?;
    let manifest_digest = digest(&manifest);
    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": INDEX_MEDIA_TYPE,
        "manifests": [{
            "mediaType": MANIFEST_MEDIA_TYPE,
            "digest": format!("sha256:{manifest_digest}"),
            "size": manifest.len(),
            "platform": {"os": os, "architecture": architecture},
        }],
    }))?;
    let archive = [
        (
            "oci-layout",
            br#"{"imageLayoutVersion":"1.0.0"}"#.as_slice(),
        ),
        ("index.json", index.as_slice()),
    ];
    let mut bytes = Vec::new();
    append(&mut bytes, archive[0].0, archive[0].1);
    append(&mut bytes, archive[1].0, archive[1].1);
    append(
        &mut bytes,
        &format!("blobs/sha256/{manifest_digest}"),
        &manifest,
    );
    append(
        &mut bytes,
        &format!("blobs/sha256/{config_digest}"),
        &stored_config,
    );
    append(&mut bytes, &format!("blobs/sha256/{layer_digest}"), &layer);
    bytes.extend_from_slice(&[0; 1024]);
    let path = directory.join(format!("{name}.oci"));
    fs::write(&path, bytes)?;
    Ok(OciImage {
        path,
        manifest_digest: format!("sha256:{manifest_digest}"),
        config_digest: format!("sha256:{config_digest}"),
    })
}

fn descriptor(media_type: &str, digest: &str, size: usize) -> Value {
    json!({"mediaType": media_type, "digest": format!("sha256:{digest}"), "size": size})
}

fn append(archive: &mut Vec<u8>, path: &str, bytes: &[u8]) {
    let mut header = [0_u8; 512];
    header[..path.len()].copy_from_slice(path.as_bytes());
    octal(&mut header[100..108], 0o644);
    octal(&mut header[108..116], 0);
    octal(&mut header[116..124], 0);
    octal(&mut header[124..136], bytes.len() as u64);
    octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let encoded = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(encoded.as_bytes());
    archive.extend_from_slice(&header);
    archive.extend_from_slice(bytes);
    let padding = (512 - bytes.len() % 512) % 512;
    archive.resize(archive.len() + padding, 0);
}

fn octal(field: &mut [u8], value: u64) {
    let encoded = format!("{value:0width$o}\0", width = field.len() - 1);
    field.copy_from_slice(encoded.as_bytes());
}

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 15)]));
    }
    value
}

fn next_filename(prefix: &str) -> String {
    let number = ARCHIVE_NUMBER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{number}")
}
