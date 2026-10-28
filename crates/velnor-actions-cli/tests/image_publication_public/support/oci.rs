// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use flate2::{Compression, GzBuilder};
use serde_json::json;
use sha2::{Digest, Sha256};
use velnor_actions_contract::ImageCandidateOci;

use super::FixtureProfile;

const CONFIG_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const INDEX_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const RAW_LAYER_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
const LAYER_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub(crate) struct Layout {
    pub(crate) archive: Vec<u8>,
    pub(crate) index: Vec<u8>,
    pub(crate) identity: ImageCandidateOci,
    pub(crate) inventory: BTreeMap<String, Vec<u8>>,
}

impl Layout {
    pub(crate) fn write_directory(&self, destination: &Path) -> Result<(), Box<dyn Error>> {
        fs::create_dir(destination)?;
        fs::write(
            destination.join("oci-layout"),
            br#"{"imageLayoutVersion":"1.0.0"}"#,
        )?;
        fs::write(destination.join("index.json"), &self.index)?;
        let blobs = destination.join("blobs");
        fs::create_dir(&blobs)?;
        let sha256 = blobs.join("sha256");
        fs::create_dir(&sha256)?;
        for (path, bytes) in &self.inventory {
            let name = Path::new(path).file_name().ok_or("blob path has no name")?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(sha256.join(name))?;
            output.write_all(bytes)?;
        }
        Ok(())
    }
}

pub(crate) fn create(source: &str) -> Result<Layout, Box<dyn Error>> {
    create_with_image_config(
        &json!({"Labels":{"org.opencontainers.image.revision":source},
        "User":"fixture","Env":["FIXTURE=1"],"Entrypoint":["/fixture"],"WorkingDir":"/"}),
    )
}

pub(crate) fn create_for(profile: FixtureProfile, source: &str) -> Result<Layout, Box<dyn Error>> {
    let config = match profile {
        FixtureProfile::Job => {
            return create(source);
        }
        FixtureProfile::Runner | FixtureProfile::Dind => {
            json!({"Labels":{"org.opencontainers.image.revision":source},"User":"fixture"})
        }
        FixtureProfile::ResourceProbe => json!({
            "Labels":{"org.opencontainers.image.revision":source},
            "User":"65532:65532",
            "Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
            "Entrypoint":["/velnor/resource-probe"],
            "WorkingDir":"/"
        }),
    };
    create_with_image_config(&config)
}

pub(crate) fn create_with_image_config(
    image_config: &serde_json::Value,
) -> Result<Layout, Box<dyn Error>> {
    create_with_layer_encoding(image_config, true)
}

pub(crate) fn create_with_raw_tar(
    image_config: &serde_json::Value,
) -> Result<Layout, Box<dyn Error>> {
    create_with_layer_encoding(image_config, false)
}

pub(crate) fn create_with_large_raw_tar(
    image_config: &serde_json::Value,
) -> Result<Layout, Box<dyn Error>> {
    let payload = (0..96 * 1024)
        .map(|index| u8::try_from(index % 251))
        .collect::<Result<Vec<_>, _>>()?;
    create_with_layer_payload(image_config, false, &payload)
}

fn create_with_layer_encoding(
    image_config: &serde_json::Value,
    gzip_layer: bool,
) -> Result<Layout, Box<dyn Error>> {
    create_with_layer_payload(
        image_config,
        gzip_layer,
        b"native publication fixture layer",
    )
}

fn create_with_layer_payload(
    image_config: &serde_json::Value,
    gzip_layer: bool,
    payload: &[u8],
) -> Result<Layout, Box<dyn Error>> {
    let mut uncompressed_layer = Vec::new();
    append(&mut uncompressed_layer, "fixture.txt", payload)?;
    uncompressed_layer.resize(uncompressed_layer.len() + 1024, 0);
    let diff_id = hex(&Sha256::digest(&uncompressed_layer));
    let (layer_type, layer) = if gzip_layer {
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), Compression::default());
        encoder.write_all(&uncompressed_layer)?;
        (LAYER_TYPE, encoder.finish()?)
    } else {
        (RAW_LAYER_TYPE, uncompressed_layer.clone())
    };
    let layer_digest = hex(&Sha256::digest(&layer));
    let config = serde_json::to_vec(
        &json!({"architecture":"amd64","os":"linux","config":image_config,
        "rootfs":{"type":"layers","diff_ids":[format!("sha256:{diff_id}")]}}),
    )?;
    let config_digest = hex(&Sha256::digest(&config));
    let manifest = serde_json::to_vec(&json!({"schemaVersion":2,"mediaType":MANIFEST_TYPE,
        "config":descriptor(CONFIG_TYPE,&config_digest,config.len()),"layers":[descriptor(layer_type,&layer_digest,layer.len())]}))?;
    let manifest_digest = hex(&Sha256::digest(&manifest));
    let index = serde_json::to_vec(
        &json!({"schemaVersion":2,"mediaType":INDEX_TYPE,"manifests":[{"mediaType":MANIFEST_TYPE,
        "digest":format!("sha256:{manifest_digest}"),"size":manifest.len(),"platform":{"os":"linux","architecture":"amd64"}}]}),
    )?;
    let index_digest = hex(&Sha256::digest(&index));
    let manifest_path = format!("blobs/sha256/{manifest_digest}");
    let config_path = format!("blobs/sha256/{config_digest}");
    let layer_path = format!("blobs/sha256/{layer_digest}");
    let inventory = BTreeMap::from([
        (manifest_path.clone(), manifest.clone()),
        (config_path.clone(), config.clone()),
        (layer_path.clone(), layer.clone()),
    ]);
    let mut archive = Vec::new();
    append(
        &mut archive,
        "oci-layout",
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
    append(&mut archive, "index.json", &index)?;
    append(&mut archive, &manifest_path, &manifest)?;
    append(&mut archive, &config_path, &config)?;
    append(&mut archive, &layer_path, &layer)?;
    archive.extend_from_slice(&[0; 1024]);
    let index_size_bytes = index.len() as u64;
    Ok(Layout {
        archive,
        index,
        identity: ImageCandidateOci {
            archive_index_digest: index_digest,
            archive_index_size_bytes: index_size_bytes,
            platform_manifest_digest: format!("sha256:{manifest_digest}"),
            platform_manifest_size_bytes: manifest.len() as u64,
            image_configuration_digest: format!("sha256:{config_digest}"),
            image_configuration_size_bytes: config.len() as u64,
            layer_digests: vec![format!("sha256:{layer_digest}")],
        },
        inventory,
    })
}

fn descriptor(media_type: &str, digest: &str, size: usize) -> serde_json::Value {
    json!({"mediaType":media_type,"digest":format!("sha256:{digest}"),"size":size})
}

fn append(archive: &mut Vec<u8>, path: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
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
    header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    archive.write_all(&header)?;
    archive.write_all(bytes)?;
    let padding = (512 - bytes.len() % 512) % 512;
    archive.resize(archive.len() + padding, 0);
    Ok(())
}

fn octal(field: &mut [u8], value: u64) {
    field.copy_from_slice(format!("{value:0width$o}\0", width = field.len() - 1).as_bytes());
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
