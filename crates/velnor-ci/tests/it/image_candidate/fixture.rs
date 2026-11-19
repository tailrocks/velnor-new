// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Read, Write};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use velnor_ci::image_candidate::archive::{Blob, scan};
use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_candidate::oci::{BlobSource, verify};

pub(super) const LAYER: &str = "application/vnd.oci.image.layer.v1.tar";
pub(super) const LAYER_GZ: &str = "application/vnd.oci.image.layer.v1.tar+gzip";

pub(super) fn digest(data: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(data))
}

pub(super) fn header(name: &str, size: usize, kind: u8) -> Vec<u8> {
    let mut block = vec![0u8; 512];
    block[..name.len()].copy_from_slice(name.as_bytes());
    block[100..107].copy_from_slice(b"0000644");
    block[108..115].copy_from_slice(b"0000000");
    block[116..123].copy_from_slice(b"0000000");
    block[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    block[136..148].copy_from_slice(b"00000000000\0");
    block[148..156].copy_from_slice(b"        ");
    block[156] = kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    block
}

pub(super) fn member(name: &str, data: &[u8], kind: u8) -> Vec<u8> {
    let mut out = header(name, data.len(), kind);
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
}

pub(super) fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = entries
        .iter()
        .flat_map(|(name, data)| member(name, data, b'0'))
        .collect();
    out.extend_from_slice(&[0u8; 1024]);
    out
}

pub(super) fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .unwrap_or_else(|e| unreachable!("{e}"));
    encoder.finish().unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) struct Image {
    pub(super) entries: Vec<(String, Vec<u8>)>,
}

pub(super) struct Spec<'a> {
    pub(super) layers: Vec<(Vec<u8>, &'a str)>,
    pub(super) diff_ids: Option<Vec<String>>,
    pub(super) platform: Value,
}

pub(super) fn spec(layers: Vec<(Vec<u8>, &str)>) -> Spec<'_> {
    Spec {
        layers,
        diff_ids: None,
        platform: json!({"architecture": "amd64", "os": "linux"}),
    }
}

pub(super) fn plain_layer() -> Vec<u8> {
    tar(&[("job-image", b"job-image-ok")])
}

pub(super) fn build(spec: &Spec<'_>) -> Image {
    let diffs: Vec<String> = spec.diff_ids.clone().unwrap_or_else(|| {
        spec.layers
            .iter()
            .map(|(data, media)| {
                if media.ends_with("+gzip") {
                    digest(&gunzip(data))
                } else {
                    digest(data)
                }
            })
            .collect()
    });
    let config = json!({"architecture": "amd64", "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": diffs}})
    .to_string()
    .into_bytes();
    let layers: Vec<Value> = spec
        .layers
        .iter()
        .map(
            |(data, media)| json!({"mediaType": media, "digest": digest(data), "size": data.len()}),
        )
        .collect();
    let manifest = json!({"schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": digest(&config), "size": config.len()},
        "layers": layers})
    .to_string()
    .into_bytes();
    let index = json!({"schemaVersion": 2, "manifests": [{
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "digest": digest(&manifest), "size": manifest.len(), "platform": spec.platform}]})
    .to_string()
    .into_bytes();
    let mut entries = vec![
        (
            "oci-layout".to_owned(),
            br#"{"imageLayoutVersion":"1.0.0"}"#.to_vec(),
        ),
        ("index.json".to_owned(), index),
    ];
    let mut blobs = vec![manifest, config];
    blobs.extend(spec.layers.iter().map(|(data, _)| data.clone()));
    for data in blobs {
        let name = format!("blobs/sha256/{}", sha256_hex(&data));
        if entries.iter().all(|(known, _)| *known != name) {
            entries.push((name, data));
        }
    }
    Image { entries }
}

pub(super) fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .unwrap_or_else(|e| unreachable!("{e}"));
    out
}

impl Image {
    pub(super) fn archive(&self) -> Vec<u8> {
        let entries: Vec<(&str, &[u8])> = self
            .entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        tar(&entries)
    }

    pub(super) fn replace(&mut self, name_of: impl Fn(&[u8]) -> bool, with: &[u8]) {
        for (_, data) in &mut self.entries {
            if name_of(data) {
                *data = with.to_vec();
            }
        }
    }
}

struct Memory(Vec<u8>);

impl BlobSource for Memory {
    fn open(&self, blob: &Blob) -> std::io::Result<Box<dyn Read + Send + '_>> {
        let start = usize::try_from(blob.offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(usize::try_from(blob.size).unwrap_or(usize::MAX));
        let slice = self.0.get(start..end).unwrap_or_default();
        Ok(Box::new(Cursor::new(slice.to_vec())))
    }
}

pub(super) fn check(archive: &[u8]) -> Result<Value, String> {
    let scanned = scan(Cursor::new(archive.to_vec())).map_err(|e| e.to_string())?;
    verify(&scanned, &Memory(archive.to_vec())).map_err(|e| e.to_string())
}

pub(super) fn refused(archive: &[u8]) -> String {
    check(archive)
        .err()
        .unwrap_or_else(|| "ACCEPTED".to_owned())
}
