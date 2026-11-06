// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::digest_bytes;
use serde_json::json;
use std::{collections::BTreeMap, error::Error, fs, io, path::Path};

pub(crate) struct Layout {
    pub(crate) manifest_digest: String,
    pub(crate) manifest: Vec<u8>,
    pub(crate) blobs: BTreeMap<String, Vec<u8>>,
    pub(crate) index: Vec<u8>,
    pub(crate) layer_digest: String,
    pub(crate) layer: Vec<u8>,
}

impl Layout {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let config =
            br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#
                .to_vec();
        let layer = b"fixture-layer-bytes".to_vec();
        let config_digest = digest_bytes(&config);
        let layer_digest = digest_bytes(&layer);
        let manifest = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType":"application/vnd.oci.image.config.v1+json","digest":config_digest,"size":config.len()},
            "layers": [{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":layer_digest,"size":layer.len()}]
        }))?;
        let manifest_digest = digest_bytes(&manifest);
        let index = serde_json::to_vec(&json!({"schemaVersion":2,"manifests":[{
            "mediaType":"application/vnd.oci.image.manifest.v1+json","digest":manifest_digest,"size":manifest.len(),
            "platform":{"os":"linux","architecture":"amd64"}
        }]}))?;
        let mut blobs = BTreeMap::new();
        blobs.insert(config_digest, config);
        blobs.insert(layer_digest.clone(), layer.clone());
        Ok(Self {
            manifest_digest,
            manifest,
            blobs,
            index,
            layer_digest,
            layer,
        })
    }

    pub(crate) fn write(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)?;
        fs::write(
            path.join("oci-layout"),
            br#"{"imageLayoutVersion":"1.0.0"}"#,
        )?;
        fs::write(path.join("index.json"), &self.index)?;
        for (digest, bytes) in &self.blobs {
            let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
            let destination = path.join("blobs/sha256");
            fs::create_dir_all(&destination)?;
            fs::write(destination.join(hex), bytes)?;
        }
        fs::write(
            path.join("blobs/sha256")
                .join(self.manifest_digest.trim_start_matches("sha256:")),
            &self.manifest,
        )?;
        Ok(())
    }
}
