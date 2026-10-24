use std::io::Cursor;

use serde_json::{Value, json};
use tar::{Builder, Header};

use super::super::unpack::unpack;
use super::{IMAGE_TAG, derive, sha256};

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const IMAGE_MANIFEST_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONFIG_SHA: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const LAYER_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[test]
fn derived_load_rewrites_only_tag_metadata_and_is_deterministic() -> Result<(), String> {
    let archive = source_archive(IMAGE_TAG, &format!("docker.io/library/{IMAGE_TAG}"))?;
    let original_sha = sha256(&archive);
    let first = derive(&archive, SOURCE_SHA, &original_sha)
        .map_err(crate::launch::resource_probe::provider::failure::ProviderFailure::code)?;
    let second = derive(&archive, SOURCE_SHA, &original_sha)
        .map_err(crate::launch::resource_probe::provider::failure::ProviderFailure::code)?;
    let reference = format!("velnor-resource-probe:v{SOURCE_SHA}-{original_sha}");
    let original_files = unpack(&archive)
        .map_err(crate::launch::resource_probe::provider::failure::ProviderFailure::code)?;
    let derived_files = unpack(&first.bytes)
        .map_err(crate::launch::resource_probe::provider::failure::ProviderFailure::code)?;

    assert_eq!(first.image_reference, reference);
    assert_eq!(second.bytes, first.bytes);
    assert_eq!(first.archive_sha256, sha256(&first.bytes));
    assert_eq!(first.index_digest, sha256(&derived_files["index.json"]));
    assert_eq!(original_files["oci-layout"], derived_files["oci-layout"]);
    for path in [
        "blobs/sha256/".to_owned() + IMAGE_MANIFEST_SHA,
        "blobs/sha256/".to_owned() + CONFIG_SHA,
        "blobs/sha256/".to_owned() + LAYER_SHA,
    ] {
        assert_eq!(original_files[&path], derived_files[&path]);
    }

    let original_index: Value =
        serde_json::from_slice(&original_files["index.json"]).map_err(|error| error.to_string())?;
    let derived_index: Value =
        serde_json::from_slice(&derived_files["index.json"]).map_err(|error| error.to_string())?;
    let mut expected_index = original_index;
    expected_index["manifests"][0]["annotations"]["io.containerd.image.name"] =
        json!(format!("docker.io/library/{reference}"));
    expected_index["manifests"][0]["annotations"]["org.opencontainers.image.ref.name"] = json!(
        reference
            .split_once(':')
            .map(|(_, tag)| tag)
            .ok_or("missing tag")?
    );
    assert_eq!(derived_index, expected_index);

    let legacy_manifest: Value = serde_json::from_slice(&derived_files["manifest.json"])
        .map_err(|error| error.to_string())?;
    assert_eq!(legacy_manifest[0]["RepoTags"], json!([reference]));
    let original_manifest: Value = serde_json::from_slice(&original_files["manifest.json"])
        .map_err(|error| error.to_string())?;
    assert_eq!(legacy_manifest[0]["Config"], original_manifest[0]["Config"]);
    assert_eq!(legacy_manifest[0]["Layers"], original_manifest[0]["Layers"]);
    Ok(())
}

#[test]
fn derived_load_rejects_unexpected_original_reference_metadata() -> Result<(), String> {
    let archive = source_archive("velnor-resource-probe:foreign", "docker.io/library/foreign")?;
    let archive_sha = sha256(&archive);
    assert!(derive(&archive, SOURCE_SHA, &archive_sha).is_err());
    Ok(())
}

fn source_archive(repo_tag: &str, image_name: &str) -> Result<Vec<u8>, String> {
    let index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": format!("sha256:{IMAGE_MANIFEST_SHA}"),
            "size": 1,
            "annotations": {
                "io.containerd.image.name": image_name,
                "org.opencontainers.image.ref.name": "linux-amd64"
            },
            "platform": { "architecture": "amd64", "os": "linux" }
        }]
    });
    let legacy = json!([{
        "Config": format!("blobs/sha256/{CONFIG_SHA}"),
        "RepoTags": [repo_tag],
        "Layers": [format!("blobs/sha256/{LAYER_SHA}")]
    }]);
    let files = [
        (
            "oci-layout".to_owned(),
            br#"{"imageLayoutVersion":"1.0.0"}"#.to_vec(),
        ),
        (
            "index.json".to_owned(),
            serde_json::to_vec(&index).map_err(|error| error.to_string())?,
        ),
        (
            "manifest.json".to_owned(),
            serde_json::to_vec(&legacy).map_err(|error| error.to_string())?,
        ),
        (
            format!("blobs/sha256/{IMAGE_MANIFEST_SHA}"),
            b"image-manifest".to_vec(),
        ),
        (format!("blobs/sha256/{CONFIG_SHA}"), b"config".to_vec()),
        (format!("blobs/sha256/{LAYER_SHA}"), b"layer".to_vec()),
    ];
    let mut builder = Builder::new(Vec::new());
    append_directory(&mut builder, "blobs/")?;
    append_directory(&mut builder, "blobs/sha256/")?;
    for (path, bytes) in files {
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(bytes.len()).map_err(|error| error.to_string())?);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, Cursor::new(bytes))
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

fn append_directory(builder: &mut Builder<Vec<u8>>, path: &str) -> Result<(), String> {
    let mut header = Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o755);
    header.set_entry_type(tar::EntryType::Directory);
    header.set_cksum();
    builder
        .append_data(&mut header, path, Cursor::new([]))
        .map_err(|error| error.to_string())
}
