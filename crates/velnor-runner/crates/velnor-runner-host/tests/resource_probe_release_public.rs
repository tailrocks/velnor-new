use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

use flate2::{Compression, GzBuilder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SOURCE: &str = "1111111111111111111111111111111111111111";
const WRONG_SOURCE: &str = "2222222222222222222222222222222222222222";
const IMAGE_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";

type TestResult<T> = Result<T, Box<dyn Error>>;

struct Fixture {
    archive: Vec<u8>,
    inspect: Vec<u8>,
    layer_bytes: Vec<u8>,
    entries: Vec<(String, Vec<u8>)>,
    index_sha256: String,
    manifest_digest: String,
    config_digest: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_bytes(&Sha256::digest(bytes)))
}

fn hex_digest(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn descriptor(bytes: &[u8], media_type: &str) -> Value {
    json!({"mediaType": media_type, "digest": digest(bytes), "size": bytes.len()})
}

fn fixture(source: &str) -> TestResult<Fixture> {
    let config = json!({
        "User": "65532:65532",
        "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
        "Entrypoint": ["/velnor/resource-probe"],
        "WorkingDir": "/",
        "Labels": {SOURCE_LABEL: source}
    });
    let config_bytes = serde_json::to_vec(&json!({
        "architecture": "amd64",
        "os": "linux",
        "config": config
    }))?;
    let config_digest = digest(&config_bytes);
    let layer_bytes = make_layer()?;
    let layer_digest = digest(&layer_bytes);
    let image_manifest = json!({
        "schemaVersion": 2,
        "mediaType": IMAGE_MEDIA_TYPE,
        "config": descriptor(&config_bytes, CONFIG_MEDIA_TYPE),
        "layers": [descriptor(&layer_bytes, LAYER_MEDIA_TYPE)]
    });
    let image_manifest_bytes = serde_json::to_vec(&image_manifest)?;
    let image_manifest_digest = digest(&image_manifest_bytes);
    let index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": IMAGE_MEDIA_TYPE,
            "digest": image_manifest_digest,
            "size": image_manifest_bytes.len(),
            "platform": {"os": "linux", "architecture": "amd64"}
        }]
    });
    let index_bytes = serde_json::to_vec(&index)?;
    let compatibility = json!([{
        "Config": blob_name(&config_digest),
        "RepoTags": [IMAGE_TAG],
        "Layers": [blob_name(&layer_digest)]
    }]);
    let entries = vec![
        (
            "oci-layout".to_owned(),
            b"{\"imageLayoutVersion\":\"1.0.0\"}".to_vec(),
        ),
        ("index.json".to_owned(), index_bytes.clone()),
        (
            "manifest.json".to_owned(),
            serde_json::to_vec(&compatibility)?,
        ),
        (blob_name(&config_digest), config_bytes),
        (blob_name(&layer_digest), layer_bytes.clone()),
        (blob_name(&image_manifest_digest), image_manifest_bytes),
    ];
    let archive = build_archive(&entries)?;
    let inspect = serde_json::to_vec(&json!([{
        "Id": config_digest,
        "Os": "linux",
        "Architecture": "amd64",
        "Config": config
    }]))?;
    Ok(Fixture {
        archive,
        inspect,
        layer_bytes,
        entries,
        index_sha256: hex_digest(&index_bytes),
        manifest_digest: image_manifest_digest,
        config_digest,
    })
}

fn blob_name(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

fn make_layer() -> TestResult<Vec<u8>> {
    let payload = b"deterministic resource-probe layer";
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o644);
    header.set_size(u64::try_from(payload.len())?);
    header.set_mtime(0);
    header.set_cksum();
    tar.append_data(&mut header, "probe", Cursor::new(payload))?;
    let tar_bytes = tar.into_inner()?;
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes)?;
    Ok(encoder.finish()?)
}

fn build_archive(entries: &[(String, Vec<u8>)]) -> TestResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    append_directories(&mut builder)?;
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(u64::try_from(bytes.len())?);
        header.set_mtime(0);
        header.set_cksum();
        builder.append_data(&mut header, name, Cursor::new(bytes))?;
    }
    Ok(builder.into_inner()?)
}

fn append_directories(builder: &mut tar::Builder<Vec<u8>>) -> TestResult<()> {
    for name in ["blobs", "blobs/sha256"] {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        header.set_mtime(0);
        header.set_cksum();
        builder.append_data(&mut header, name, Cursor::new([]))?;
    }
    Ok(())
}

fn run_write_manifest(
    temp: &tempfile::TempDir,
    archive: &[u8],
    inspect: &[u8],
    name: &str,
) -> TestResult<(Output, PathBuf)> {
    let archive_path = temp.path().join(format!("{name}.tar"));
    let inspect_path = temp.path().join(format!("{name}.json"));
    let output_path = temp.path().join(format!("{name}-manifest.json"));
    fs::write(&archive_path, archive)?;
    fs::write(&inspect_path, inspect)?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .env_clear()
        .env("VELNOR_SOURCE_SHA", SOURCE)
        .env("VELNOR_WORKFLOW_AUTHORITY_SHA", SOURCE)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_SHA", SOURCE)
        .arg("write-manifest")
        .arg(inspect_path)
        .arg(archive_path)
        .arg(&output_path)
        .output()?;
    Ok((output, output_path))
}

#[test]
fn public_write_manifest_admits_matching_oci_archive_and_inspect() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let fixture = fixture(SOURCE)?;
    let (output, manifest_path) =
        run_write_manifest(&temp, &fixture.archive, &fixture.inspect, "valid")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    assert_eq!(manifest["repository"], "tailrocks/velnor-new");
    assert_eq!(manifest["source_ref"], "refs/heads/main");
    assert_eq!(manifest["source_commit"], SOURCE);
    assert_eq!(manifest["workflow_authority_sha"], SOURCE);
    assert_eq!(manifest["platform"], "linux/amd64");
    assert_eq!(manifest["oci_index_sha256"], fixture.index_sha256);
    assert_eq!(manifest["image_manifest_digest"], fixture.manifest_digest);
    assert_eq!(manifest["config_digest"], fixture.config_digest);
    assert_eq!(manifest["archive_sha256"], hex_digest(&fixture.archive));
    assert_eq!(manifest["image_user"], "65532:65532");
    assert_eq!(manifest["entrypoint"], "/velnor/resource-probe");
    Ok(())
}

#[test]
fn public_write_manifest_rejects_duplicate_oci_member_without_output() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let fixture = fixture(SOURCE)?;
    let mut entries = fixture.entries.clone();
    let index = entries
        .iter()
        .find(|(name, _)| name == "index.json")
        .map(|(_, bytes)| bytes.clone())
        .ok_or_else(|| std::io::Error::other("valid fixture contains index.json"))?;
    entries.push(("index.json".to_owned(), index));
    let duplicate = build_archive(&entries)?;
    let (output, manifest_path) =
        run_write_manifest(&temp, &duplicate, &fixture.inspect, "duplicate")?;
    assert!(!output.status.success());
    assert!(!manifest_path.exists());
    Ok(())
}

#[test]
fn public_write_manifest_rejects_blob_bytes_that_do_not_match_their_digest_name() -> TestResult<()>
{
    let temp = tempfile::tempdir()?;
    let fixture = fixture(SOURCE)?;
    let mut archive = fixture.archive.clone();
    let offset = archive
        .windows(fixture.layer_bytes.len())
        .position(|window| window == fixture.layer_bytes.as_slice())
        .ok_or_else(|| std::io::Error::other("locate layer bytes"))?;
    archive[offset] ^= 1;
    let (output, manifest_path) =
        run_write_manifest(&temp, &archive, &fixture.inspect, "tampered")?;
    assert!(!output.status.success());
    assert!(!manifest_path.exists());
    Ok(())
}

#[test]
fn public_write_manifest_rejects_rehashed_image_with_wrong_source_label() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let fixture = fixture(WRONG_SOURCE)?;
    let (output, manifest_path) =
        run_write_manifest(&temp, &fixture.archive, &fixture.inspect, "wrong-source")?;
    assert!(!output.status.success());
    assert!(!manifest_path.exists());
    Ok(())
}
