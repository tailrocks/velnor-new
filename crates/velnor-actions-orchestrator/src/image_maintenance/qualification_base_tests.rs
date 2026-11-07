// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare_saved;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::Duration,
};
use velnor_actions_mise::CheckDeadline;

const FIXTURE: &[u8] = include_bytes!("qualification_base_tests/tiny-save.tar");
const DIGEST: &str = "sha256:f9103977e68e16cefd135938958f735a52092d95d7e8dbef76b2dc69c0a420ac";
type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Entries = BTreeMap<String, Vec<u8>>;

#[test]
fn docker_saved_image_preserves_authenticated_blobs() -> Result<()> {
    let original = entries()?;
    let (directory, layout) = attempt(FIXTURE, DIGEST)?;
    for (name, bytes) in original
        .iter()
        .filter(|(name, _)| name.starts_with("blobs/sha256/"))
    {
        assert_eq!(&fs::read(layout.join(name))?, bytes);
    }
    let evidence = velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_oci_candidate(
        &directory.path().join("normalized.tar"),
    )?;
    assert_eq!(evidence.platform_manifest_digest, DIGEST);
    Ok(())
}

#[test]
fn wrong_or_missing_manifest_and_corrupt_blob_are_rejected() -> Result<()> {
    assert!(attempt(FIXTURE, &format!("sha256:{}", "0".repeat(64))).is_err());
    let mut missing = entries()?;
    missing.remove("index.json");
    assert!(attempt(&archive(&missing, None)?, DIGEST).is_err());
    let mut no_manifest = entries()?;
    let mut index: serde_json::Value = serde_json::from_slice(&no_manifest["index.json"])?;
    index["manifests"] = serde_json::json!([]);
    no_manifest.insert("index.json".into(), serde_json::to_vec(&index)?);
    assert!(attempt(&archive(&no_manifest, None)?, DIGEST).is_err());
    let mut malformed = entries()?;
    malformed.insert("index.json".into(), b"{".to_vec());
    assert!(attempt(&archive(&malformed, None)?, DIGEST).is_err());
    let mut wrong = entries()?;
    let mut index: serde_json::Value = serde_json::from_slice(&wrong["index.json"])?;
    index["manifests"][0]["digest"] = serde_json::json!(format!("sha256:{}", "0".repeat(64)));
    wrong.insert("index.json".into(), serde_json::to_vec(&index)?);
    assert!(attempt(&archive(&wrong, None)?, DIGEST).is_err());
    let mut damaged = entries()?;
    let key = damaged
        .keys()
        .find(|key| key.starts_with("blobs/sha256/") && !key.ends_with(&DIGEST[7..]))
        .ok_or("missing blob")?
        .clone();
    damaged.get_mut(&key).ok_or("missing blob")?[0] ^= 1;
    let error = attempt(&archive(&damaged, None)?, DIGEST)
        .err()
        .ok_or("corrupt blob accepted")?;
    assert!(error.to_string().contains("native_base_oci"), "{error}");
    Ok(())
}

#[test]
fn unsafe_duplicate_and_oversized_entries_are_rejected() -> Result<()> {
    let original = entries()?;
    for (extra, expected) in [
        (Extra::Duplicate, "native_base_duplicate"),
        (Extra::Traversal, "native_base_path"),
        (Extra::Symlink, "native_base_entry"),
    ] {
        let error = attempt(&archive(&original, Some(extra))?, DIGEST)
            .err()
            .ok_or("unsafe entry accepted")?;
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut oversized = original;
    oversized.insert("index.json".into(), vec![b' '; 1024 * 1024 + 1]);
    assert!(attempt(&archive(&oversized, None)?, DIGEST).is_err());
    Ok(())
}

#[test]
fn rehashed_wrong_platform_is_rejected_by_the_guard() -> Result<()> {
    let mut source = entries()?;
    let manifest_key = format!("blobs/sha256/{}", &DIGEST[7..]);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&source.remove(&manifest_key).ok_or("manifest")?)?;
    let config_digest = manifest["config"]["digest"]
        .as_str()
        .ok_or("config digest")?;
    let config_key = format!("blobs/sha256/{}", &config_digest[7..]);
    let mut config: serde_json::Value =
        serde_json::from_slice(&source.remove(&config_key).ok_or("config")?)?;
    config["architecture"] = serde_json::json!("arm64");
    let config_bytes = serde_json::to_vec(&config)?;
    let config_hash = digest(&config_bytes);
    manifest["config"]["digest"] = serde_json::json!(config_hash);
    manifest["config"]["size"] = serde_json::json!(config_bytes.len());
    source.insert(format!("blobs/sha256/{}", &config_hash[7..]), config_bytes);
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let new_digest = digest(&manifest_bytes);
    let mut index: serde_json::Value = serde_json::from_slice(&source["index.json"])?;
    index["manifests"][0]["digest"] = serde_json::json!(new_digest);
    index["manifests"][0]["size"] = serde_json::json!(manifest_bytes.len());
    source.insert("index.json".into(), serde_json::to_vec(&index)?);
    source.insert(format!("blobs/sha256/{}", &new_digest[7..]), manifest_bytes);
    let error = attempt(&archive(&source, None)?, &new_digest)
        .err()
        .ok_or("wrong platform accepted")?;
    assert!(error.to_string().contains("native_base_oci"), "{error}");
    Ok(())
}

fn attempt(bytes: &[u8], expected: &str) -> Result<(tempfile::TempDir, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("input.tar");
    fs::write(&source, bytes)?;
    let layout = prepare_saved(
        &source,
        directory.path(),
        expected,
        CheckDeadline::after(Duration::from_secs(30))?,
    )?;
    Ok((directory, layout))
}

fn entries() -> Result<Entries> {
    let mut entries = Entries::new();
    for entry in tar::Archive::new(Cursor::new(FIXTURE)).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path()?.to_str().ok_or("path")?.to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        entries.insert(name, bytes);
    }
    Ok(entries)
}

#[derive(Clone, Copy)]
enum Extra {
    Duplicate,
    Traversal,
    Symlink,
}

fn archive(entries: &Entries, extra: Option<Extra>) -> Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, bytes) in entries {
        append(&mut builder, name, bytes)?;
    }
    match extra {
        Some(Extra::Duplicate) => append(&mut builder, "index.json", &entries["index.json"])?,
        Some(Extra::Traversal) => {
            let mut header = tar::Header::new_gnu();
            header.as_mut_bytes()[..9].copy_from_slice(b"../escape");
            header.set_size(0);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, Cursor::new(Vec::<u8>::new()))?;
        }
        Some(Extra::Symlink) => {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("index.json")?;
            header.set_cksum();
            builder.append_data(
                &mut header,
                format!("blobs/sha256/{}", "0".repeat(64)),
                Cursor::new(Vec::<u8>::new()),
            )?;
        }
        None => {}
    }
    Ok(builder.into_inner()?)
}

fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, bytes: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, Path::new(path), bytes)?;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    let bytes = Sha256::digest(bytes);
    let mut result = String::from("sha256:");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut result, "{byte:02x}").expect("write to string");
    }
    result
}
