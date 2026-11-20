// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{self, Read};

use serde_json::{Value, json};

use super::archive::{Blob, Scanned};
use super::hashing::sha256_hex;
use super::json::{is_digest, parse, uint};
use crate::error::CiError;

mod layers;
use layers::{check_layers, diff_ids};

const MAX_UNCOMPRESSED: u64 = 4 << 30;
const MAX_LAYER_MEMBERS: usize = 1_000_000;
const MAX_METADATA_BLOB: u64 = 64 << 20;
const MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const INDEX_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const CONFIG_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const LAYER_TYPES: [&str; 4] = [
    "application/vnd.oci.image.layer.v1.tar",
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
];

pub trait BlobSource: Sync {
    fn open(&self, blob: &Blob) -> io::Result<Box<dyn Read + Send + '_>>;
}

#[derive(Debug, Clone)]
struct Descriptor {
    digest: String,
    hex: String,
    size: u64,
    media_type: String,
}

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn descriptor(value: Option<&Value>, label: &str) -> Result<Descriptor, CiError> {
    let object = value.and_then(Value::as_object).filter(|map| {
        ["mediaType", "digest", "size"]
            .iter()
            .all(|key| map.contains_key(*key))
    });
    let Some(object) = object else {
        return fail(format!("OCI {label} descriptor is incorrect."));
    };
    if !is_digest(&object["digest"]) {
        return fail(format!(
            "OCI {label} digest must be a lowercase SHA-256 digest."
        ));
    }
    let Some(size) = uint(&object["size"]) else {
        return fail(format!("OCI {label} size is incorrect."));
    };
    let Some(media_type) = object["mediaType"].as_str() else {
        return fail(format!("OCI {label} media type is incorrect."));
    };
    let digest = object["digest"].as_str().unwrap_or_default().to_owned();
    Ok(Descriptor {
        hex: digest.trim_start_matches("sha256:").to_owned(),
        digest,
        size,
        media_type: media_type.to_owned(),
    })
}

fn record_blob(
    referenced: &mut BTreeMap<String, (u64, String)>,
    hex: &str,
    size: u64,
    media: &str,
) -> Result<(), CiError> {
    let metadata = (size, media.to_owned());
    match referenced.get(hex) {
        Some(existing) if *existing != metadata => {
            fail("OCI descriptors for the same blob have different size or media type values.")
        }
        _ => {
            referenced.insert(hex.to_owned(), metadata);
            Ok(())
        }
    }
}

fn read_metadata(source: &dyn BlobSource, blob: &Blob) -> Result<Vec<u8>, CiError> {
    if blob.size > MAX_METADATA_BLOB {
        return fail("OCI metadata blob is too large.");
    }
    let mut data = Vec::new();
    source
        .open(blob)
        .and_then(|mut stream| stream.read_to_end(&mut data))
        .map_err(|_| CiError::new("Cannot read OCI archive."))?;
    Ok(data)
}

fn check_index(scan: &Scanned) -> Result<(Descriptor, Value), CiError> {
    let layout = parse(&scan.layout, "OCI layout")?;
    if layout
        .as_object()
        .is_none_or(|map| map.len() != 1 || !map.contains_key("imageLayoutVersion"))
    {
        return fail("OCI layout has unknown or missing fields.");
    }
    if layout["imageLayoutVersion"] != "1.0.0" {
        return fail("Unsupported OCI layout version.");
    }
    let index = parse(&scan.index, "OCI index")?;
    let allowed = ["schemaVersion", "mediaType", "manifests", "annotations"];
    let Some(map) = index
        .as_object()
        .filter(|map| map.keys().all(|key| allowed.contains(&key.as_str())))
    else {
        return fail("OCI index has unknown fields.");
    };
    let manifests = map.get("manifests").and_then(Value::as_array);
    if map.get("schemaVersion").and_then(uint) != Some(2) || manifests.is_none() {
        return fail("OCI index schema is incorrect.");
    }
    let manifests = manifests.map_or(&[][..], Vec::as_slice);
    if manifests.len() != 1 {
        return fail("OCI index must select one platform manifest.");
    }
    let entry = manifests[0].as_object().filter(|entry| {
        let required = ["mediaType", "digest", "size", "platform"];
        required.iter().all(|key| entry.contains_key(*key))
            && entry
                .keys()
                .all(|key| required.contains(&key.as_str()) || key == "annotations")
    });
    let Some(entry) = entry else {
        return fail("OCI platform descriptor has unknown or missing fields.");
    };
    if uint(&entry["size"]).is_none_or(|size| size < 1) {
        return fail("OCI manifest size is incorrect.");
    }
    if map.get("mediaType").unwrap_or(&json!(INDEX_TYPE)) != INDEX_TYPE {
        return fail("OCI index media type is incorrect.");
    }
    if entry["platform"] != json!({"architecture": "amd64", "os": "linux"}) {
        return fail("OCI archive platform is not linux/amd64.");
    }
    if !is_digest(&entry["digest"]) {
        return fail("Manifest digest must be a lowercase SHA-256 digest.");
    }
    let digest = entry["digest"].as_str().unwrap_or_default().to_owned();
    let media = entry["mediaType"].clone();
    let described = Descriptor {
        hex: digest.trim_start_matches("sha256:").to_owned(),
        digest,
        size: uint(&entry["size"]).unwrap_or_default(),
        media_type: media.as_str().unwrap_or_default().to_owned(),
    };
    Ok((described, media))
}

struct Graph {
    described: Descriptor,
    manifest_raw: Vec<u8>,
    config: Descriptor,
    entries: Vec<Descriptor>,
    referenced: BTreeMap<String, (u64, String)>,
}

fn read_graph(scan: &Scanned, source: &dyn BlobSource) -> Result<Graph, CiError> {
    let (described, described_media) = check_index(scan)?;
    let Some(manifest_blob) = scan.blobs.get(&described.hex) else {
        return fail("OCI manifest blob is missing.");
    };
    if manifest_blob.size != described.size {
        return fail("OCI manifest size is not the same as its descriptor size.");
    }
    let manifest_raw = read_metadata(source, manifest_blob)?;
    if sha256_hex(&manifest_raw) != described.hex {
        return fail("OCI manifest blob has an incorrect digest.");
    }
    let manifest = parse(&manifest_raw, "OCI manifest")?;
    if manifest.get("schemaVersion").and_then(uint) != Some(2) {
        return fail("OCI manifest schema is incorrect.");
    }
    if manifest["mediaType"] != MANIFEST_TYPE {
        return fail("OCI manifest media type is incorrect.");
    }
    if described_media != manifest["mediaType"] {
        return fail("OCI index and manifest media types are different.");
    }
    let allowed = [
        "schemaVersion",
        "mediaType",
        "config",
        "layers",
        "annotations",
    ];
    if manifest
        .as_object()
        .is_none_or(|map| map.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return fail("OCI manifest has unknown fields.");
    }
    let config = descriptor(manifest.get("config"), "config")?;
    if config.media_type != CONFIG_TYPE {
        return fail("OCI image configuration media type is incorrect.");
    }
    let layers = manifest
        .get("layers")
        .and_then(Value::as_array)
        .filter(|layers| !layers.is_empty());
    let Some(layers) = layers else {
        return fail("OCI manifest must contain image layers.");
    };
    let mut referenced = BTreeMap::new();
    record_blob(
        &mut referenced,
        &described.hex,
        manifest_raw.len() as u64,
        MANIFEST_TYPE,
    )?;
    record_blob(
        &mut referenced,
        &config.hex,
        config.size,
        &config.media_type,
    )?;
    let mut entries = Vec::with_capacity(layers.len());
    for layer in layers {
        let entry = descriptor(Some(layer), "layer")?;
        if !LAYER_TYPES.contains(&entry.media_type.as_str()) {
            return fail("OCI layer media type is incorrect.");
        }
        record_blob(&mut referenced, &entry.hex, entry.size, &entry.media_type)?;
        entries.push(entry);
    }
    Ok(Graph {
        described,
        manifest_raw,
        config,
        entries,
        referenced,
    })
}

pub fn verify(scan: &Scanned, source: &dyn BlobSource) -> Result<Value, CiError> {
    let Graph {
        described,
        manifest_raw,
        config,
        entries,
        referenced,
    } = read_graph(scan, source)?;
    if scan.blobs.keys().ne(referenced.keys()) {
        return fail("OCI archive has missing or unreferenced blobs.");
    }
    if referenced
        .iter()
        .any(|(hex, (size, _))| scan.blobs[hex].size != *size)
    {
        return fail("OCI blob size is not the same as its descriptor size.");
    }
    let config_raw = read_metadata(source, &scan.blobs[&config.hex])?;
    if sha256_hex(&config_raw) != config.hex {
        return fail("OCI image configuration blob has an incorrect digest.");
    }
    let config_data = parse(&config_raw, "OCI image configuration")?;
    if config_data.get("os") != Some(&json!("linux"))
        || config_data.get("architecture") != Some(&json!("amd64"))
    {
        return fail("OCI image configuration platform is not linux/amd64.");
    }
    let diff_ids = diff_ids(&config_data, entries.len())?;
    check_layers(scan, source, &entries, &diff_ids)?;
    for hex in referenced.keys() {
        if *hex != described.hex && *hex != config.hex && scan.blobs[hex].sha != *hex {
            return fail(
                "OCI blob bytes are not the same as the bytes identified by their descriptor.",
            );
        }
    }
    Ok(json!({
        "archive_index_digest": sha256_hex(&scan.index),
        "archive_index_size_bytes": scan.index.len(),
        "platform_manifest_digest": described.digest,
        "platform_manifest_size_bytes": manifest_raw.len(),
        "image_configuration_digest": config.digest,
        "image_configuration_size_bytes": config_raw.len(),
        "layer_digests": entries.iter().map(|entry| entry.digest.clone()).collect::<Vec<_>>(),
    }))
}
