// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, fs, path::Path};

use sha2::{Digest, Sha256};

use super::hex;

pub(super) fn image_inventory(
    layout: &Path,
) -> Result<(String, serde_json::Value), Box<dyn Error>> {
    if !layout.join("oci-layout").is_file() {
        return Err("Crane received an incomplete OCI layout".into());
    }
    let index: serde_json::Value = serde_json::from_slice(&fs::read(layout.join("index.json"))?)?;
    let manifests = index["manifests"]
        .as_array()
        .filter(|manifests| manifests.len() == 1)
        .ok_or("OCI layout must have one platform manifest")?;
    let descriptor = &manifests[0];
    let digest = descriptor["digest"]
        .as_str()
        .ok_or("OCI manifest digest is missing")?;
    let size = descriptor["size"]
        .as_u64()
        .ok_or("OCI manifest size is missing")?;
    let mut inventory = serde_json::Map::new();
    for entry in fs::read_dir(layout.join("blobs/sha256"))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("OCI blob is not a regular file".into());
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid OCI blob name")?;
        let bytes = fs::read(entry.path())?;
        let blob_digest = format!("sha256:{}", hex(&Sha256::digest(&bytes)));
        if blob_digest != format!("sha256:{name}") {
            return Err("OCI blob path does not match its bytes".into());
        }
        inventory.insert(
            format!("blobs/sha256/{name}"),
            serde_json::json!({"digest":blob_digest,"size":bytes.len(),"bytes":bytes}),
        );
    }
    let inventory = serde_json::Value::Object(inventory);
    inventory_blob(&inventory, digest, size)?;
    validate_remote_inventory(&inventory, digest)?;
    Ok((digest.to_owned(), inventory))
}

pub(super) fn validate_remote_inventory(
    inventory: &serde_json::Value,
    manifest_digest: &str,
) -> Result<(), Box<dyn Error>> {
    let manifest_size = inventory_blob_size(inventory, manifest_digest)?;
    let manifest_bytes = inventory_blob(inventory, manifest_digest, manifest_size)?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let config = &manifest["config"];
    let config_digest = config["digest"]
        .as_str()
        .ok_or("remote config digest is missing")?;
    let config_size = config["size"]
        .as_u64()
        .ok_or("remote config size is missing")?;
    inventory_blob(inventory, config_digest, config_size)?;
    let layers = manifest["layers"]
        .as_array()
        .filter(|layers| !layers.is_empty())
        .ok_or("remote layers are missing")?;
    let mut referenced = BTreeSet::from([blob_path(manifest_digest)?, blob_path(config_digest)?]);
    for layer in layers {
        let digest = layer["digest"]
            .as_str()
            .ok_or("remote layer digest is missing")?;
        let size = layer["size"]
            .as_u64()
            .ok_or("remote layer size is missing")?;
        inventory_blob(inventory, digest, size)?;
        referenced.insert(blob_path(digest)?);
    }
    let actual = inventory
        .as_object()
        .ok_or("remote blob inventory is missing")?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if actual != referenced {
        return Err("remote blob inventory does not match its manifest".into());
    }
    Ok(())
}

pub(super) fn corrupt_inventory(
    inventory: &mut serde_json::Value,
    manifest_digest: &str,
    fault: &serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    let kind = fault["kind"]
        .as_str()
        .ok_or("blob fault target is missing")?;
    let digest = target_blob_digest(inventory, manifest_digest, kind)?;
    let path = blob_path(&digest)?;
    if fault["mode"] == "missing" {
        inventory
            .as_object_mut()
            .and_then(|entries| entries.remove(&path))
            .ok_or("target blob is missing")?;
        return Ok(());
    }
    if fault["mode"] == "nonzero" {
        return Ok(());
    }
    let size = {
        let bytes = inventory[&path]["bytes"]
            .as_array_mut()
            .ok_or("blob bytes are missing")?;
        match fault["mode"].as_str().ok_or("blob fault mode is missing")? {
            "short" => {
                bytes.pop().ok_or("blob bytes are empty")?;
            }
            "oversize" => bytes.push(serde_json::json!(0)),
            "corrupt" => {
                let offset = usize::try_from(
                    fault["offset"]
                        .as_u64()
                        .ok_or("blob fault offset is missing")?,
                )?;
                let byte = bytes
                    .get_mut(offset)
                    .ok_or("blob fault offset is out of range")?;
                *byte = serde_json::json!(byte.as_u64().ok_or("blob byte is invalid")? ^ 1);
            }
            _ => return Err("unsupported blob fault mode".into()),
        }
        bytes.len()
    };
    inventory[&path]["size"] = serde_json::json!(size);
    Ok(())
}

pub(super) fn is_blob_fault_target(
    inventory: &serde_json::Value,
    manifest_digest: &str,
    fault: &serde_json::Value,
    digest: &str,
) -> Result<bool, Box<dyn Error>> {
    let kind = fault["kind"]
        .as_str()
        .ok_or("blob fault target is missing")?;
    Ok(target_blob_digest(inventory, manifest_digest, kind)? == digest)
}

fn target_blob_digest(
    inventory: &serde_json::Value,
    manifest_digest: &str,
    kind: &str,
) -> Result<String, Box<dyn Error>> {
    let size = inventory_blob_size(inventory, manifest_digest)?;
    let bytes = inventory_blob(inventory, manifest_digest, size)?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    let digest = match kind {
        "config" => manifest["config"]["digest"]
            .as_str()
            .ok_or("config digest is missing")?,
        "layer" => manifest["layers"]
            .as_array()
            .and_then(|layers| layers.first())
            .and_then(|layer| layer["digest"].as_str())
            .ok_or("layer digest is missing")?,
        _ => return Err("unsupported corruption target".into()),
    };
    Ok(digest.to_owned())
}

pub(super) fn inventory_blob_size(
    inventory: &serde_json::Value,
    digest: &str,
) -> Result<u64, Box<dyn Error>> {
    let path = blob_path(digest)?;
    inventory[&path]["size"]
        .as_u64()
        .ok_or_else(|| "OCI blob size is missing".into())
}

pub(super) fn inventory_blob_count(
    inventory: &serde_json::Value,
    manifest_digest: &str,
) -> Result<u64, Box<dyn Error>> {
    let size = inventory_blob_size(inventory, manifest_digest)?;
    let bytes = inventory_blob(inventory, manifest_digest, size)?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    let layers = manifest["layers"]
        .as_array()
        .ok_or("remote manifest has no layers")?;
    u64::try_from(layers.len().saturating_add(1)).map_err(Into::into)
}

pub(super) fn inventory_blob(
    inventory: &serde_json::Value,
    digest: &str,
    size: u64,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = blob_path(digest)?;
    let entry = &inventory[&path];
    if entry["digest"].as_str() != Some(digest) || entry["size"].as_u64() != Some(size) {
        return Err(format!("remote blob metadata mismatch: {path}").into());
    }
    let bytes = raw_inventory_blob(inventory, digest)?;
    if bytes.len() as u64 != size || format!("sha256:{}", hex(&Sha256::digest(&bytes))) != digest {
        return Err(format!("remote blob bytes do not match digest: {path}").into());
    }
    Ok(bytes)
}

pub(super) fn raw_inventory_blob(
    inventory: &serde_json::Value,
    digest: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = blob_path(digest)?;
    let entry = &inventory[&path];
    let mut bytes = Vec::new();
    for byte in entry["bytes"]
        .as_array()
        .ok_or("remote blob bytes are missing")?
    {
        bytes.push(u8::try_from(
            byte.as_u64().ok_or("remote blob byte is invalid")?,
        )?);
    }
    Ok(bytes)
}

fn blob_path(digest: &str) -> Result<String, Box<dyn Error>> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or("invalid blob digest")?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid blob digest".into());
    }
    Ok(format!("blobs/sha256/{hex}"))
}
