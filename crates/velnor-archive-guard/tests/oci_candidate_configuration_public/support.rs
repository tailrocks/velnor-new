// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::Path,
};

use serde_json::json;
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use tempfile::TempDir;

const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub(super) struct OciFixture {
    _directory: TempDir,
    pub(super) path: std::path::PathBuf,
}

pub(super) fn fixture(projection: &str, tamper_config: bool) -> Result<OciFixture, Box<dyn Error>> {
    fixture_with_metadata(projection, tamper_config, false)
}

pub(super) fn fixture_with_noncanonical_metadata(
    projection: &str,
    tamper_config: bool,
) -> Result<OciFixture, Box<dyn Error>> {
    fixture_with_metadata(projection, tamper_config, true)
}

fn fixture_with_metadata(
    projection: &str,
    tamper_config: bool,
    noncanonical_metadata: bool,
) -> Result<OciFixture, Box<dyn Error>> {
    let layer = empty_layer()?;
    let config = image_config(projection, &layer);
    let stored_config = if tamper_config {
        let mut changed = config.clone();
        let index = changed
            .windows(6)
            .position(|window| window == b"runner")
            .ok_or("configuration fixture lacks user")?;
        *changed
            .get_mut(index + 4)
            .ok_or("configuration fixture user is truncated")? = b'f';
        changed
    } else {
        config.clone()
    };
    write_archive(&config, &stored_config, &layer, noncanonical_metadata)
}

pub(super) fn archive_files(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut files = BTreeMap::new();
    for result in archive.entries()? {
        let mut entry = result?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let name = entry
            .path()?
            .to_str()
            .ok_or("fixture path is not UTF-8")?
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    Ok(files)
}

pub(super) fn change_layout_version(path: &Path) -> Result<String, Box<dyn Error>> {
    const ORIGINAL: &[u8] = b"\"imageLayoutVersion\":\"1.0.0\"";
    const CHANGED: &[u8] = b"\"imageLayoutVersion\":\"2.0.0\"";
    if ORIGINAL.len() != CHANGED.len() {
        return Err("layout fixture versions differ in length".into());
    }
    let mut bytes = fs::read(path)?;
    let mut matches = bytes
        .windows(ORIGINAL.len())
        .enumerate()
        .filter(|(_, window)| *window == ORIGINAL);
    let index = matches
        .next()
        .map(|(index, _)| index)
        .ok_or("layout fixture version is missing")?;
    if matches.next().is_some() {
        return Err("layout fixture version is ambiguous".into());
    }
    bytes[index..index + ORIGINAL.len()].copy_from_slice(CHANGED);
    let mut file = File::options().write(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(digest(&bytes))
}

fn image_config(projection: &str, layer: &[u8]) -> Vec<u8> {
    let diff_id = digest(layer);
    format!(
        "{{\"architecture\":\"amd64\",\"os\":\"linux\",\"config\":{projection},\"rootfs\":{{\"type\":\"layers\",\"diff_ids\":[\"sha256:{diff_id}\"]}}}}"
    )
    .into_bytes()
}

fn write_archive(
    config: &[u8],
    stored_config: &[u8],
    layer: &[u8],
    noncanonical_metadata: bool,
) -> Result<OciFixture, Box<dyn Error>> {
    let config_digest = digest(config);
    let layer_digest = digest(layer);
    let manifest_value = json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": descriptor(CONFIG_MEDIA_TYPE, &config_digest, config.len()),
        "layers": [descriptor(LAYER_MEDIA_TYPE, &layer_digest, layer.len())],
    });
    let manifest = if noncanonical_metadata {
        serde_json::to_vec_pretty(&manifest_value)?
    } else {
        serde_json::to_vec(&manifest_value)?
    };
    let manifest_digest = digest(&manifest);
    let index_value = json!({
        "schemaVersion": 2,
        "mediaType": INDEX_MEDIA_TYPE,
        "manifests": [{
            "mediaType": MANIFEST_MEDIA_TYPE,
            "digest": format!("sha256:{manifest_digest}"),
            "size": manifest.len(),
            "platform": {"os": "linux", "architecture": "amd64"},
        }],
    });
    let index = if noncanonical_metadata {
        serde_json::to_vec_pretty(&index_value)?
    } else {
        serde_json::to_vec(&index_value)?
    };
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("candidate.oci");
    let file = File::create(&path)?;
    let mut archive = Builder::new(file);
    let layout = if noncanonical_metadata {
        &b"{\n  \"imageLayoutVersion\" : \"1.0.0\"\n}\n"[..]
    } else {
        &br#"{"imageLayoutVersion":"1.0.0"}"#[..]
    };
    append(&mut archive, "oci-layout", layout)?;
    append(&mut archive, "index.json", &index)?;
    append(
        &mut archive,
        &format!("blobs/sha256/{manifest_digest}"),
        &manifest,
    )?;
    append(
        &mut archive,
        &format!("blobs/sha256/{config_digest}"),
        stored_config,
    )?;
    append(&mut archive, &format!("blobs/sha256/{layer_digest}"), layer)?;
    archive.finish()?;
    Ok(OciFixture {
        _directory: directory,
        path,
    })
}

fn descriptor(media_type: &str, digest: &str, size: usize) -> serde_json::Value {
    json!({
        "mediaType": media_type,
        "digest": format!("sha256:{digest}"),
        "size": size,
    })
}

fn append<W: std::io::Write>(
    archive: &mut Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut header = Header::new_ustar();
    header.set_size(u64::try_from(bytes.len())?);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, path, Cursor::new(bytes))?;
    Ok(())
}

fn empty_layer() -> Result<Vec<u8>, Box<dyn Error>> {
    let mut layer = Builder::new(Vec::new());
    layer.finish()?;
    Ok(layer.into_inner()?)
}

fn digest(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
