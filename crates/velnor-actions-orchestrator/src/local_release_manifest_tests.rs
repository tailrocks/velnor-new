//! Unit coverage for bounded local release-manifest verification.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::{sha256_nonempty_file, verify_local_generator_release_manifest};

const SOURCE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LINUX_BYTES: &[u8] = b"linux-binary\n";
const MACOS_BYTES: &[u8] = b"macos-binary\n";
const LINUX_SHA256: &str = "c8129af264901ca76e037945f033c1dc7b05f000e2eeec96d74ea0a69d9bfb61";
const MACOS_SHA256: &str = "6e851e6fefc18be0de3030587e30b1a1130f115473d34f264235735c6323b3eb";
const MACOS_X64_SHA256: &str = "b9ce8bf39067b6ba098879151d887bcadef7e1f4049131e0e703ba6ab37f6647";

struct Fixture {
    directory: TempDir,
    manifest: PathBuf,
    linux: PathBuf,
    macos: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let directory = TempDir::new()?;
        let manifest = directory.path().join("manifest.json");
        let linux = directory.path().join("linux-generator");
        let macos = directory.path().join("macos-generator");
        fs::write(&linux, LINUX_BYTES)?;
        fs::write(&macos, MACOS_BYTES)?;
        let fixture = Self {
            directory,
            manifest,
            linux,
            macos,
        };
        fixture.write_document(&valid_document())?;
        Ok(fixture)
    }

    fn write_document(&self, document: &Value) -> Result<(), Box<dyn Error>> {
        fs::write(&self.manifest, serde_json::to_vec(document)?)?;
        Ok(())
    }

    fn write_manifest_bytes(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        fs::write(&self.manifest, bytes)?;
        Ok(())
    }

    fn verify(&self, commit: &str) -> Result<(), crate::OrchestratorError> {
        verify_local_generator_release_manifest(&self.manifest, commit, &self.linux, &self.macos)
    }
}

fn valid_document() -> Value {
    json!({
        "schema": 1,
        "version": "1.2.3",
        "repository": "tailrocks/velnor-new",
        "commit": SOURCE_COMMIT,
        "targets": [
            {
                "target": "aarch64-apple-darwin",
                "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-aarch64-apple-darwin",
                "sha256": MACOS_SHA256
            },
            {
                "target": "x86_64-unknown-linux-gnu",
                "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-x86_64-unknown-linux-gnu",
                "sha256": LINUX_SHA256
            },
            {
                "target": "x86_64-apple-darwin",
                "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-x86_64-apple-darwin",
                "sha256": MACOS_X64_SHA256
            }
        ]
    })
}

fn reject(fixture: &Fixture, commit: &str) {
    assert!(fixture.verify(commit).is_err());
}

#[test]
fn valid_manifest_checks_both_target_files_independent_of_record_order()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let before_linux = fs::read(&fixture.linux)?;
    let before_macos = fs::read(&fixture.macos)?;
    assert!(fixture.verify(SOURCE_COMMIT).is_ok());
    assert_eq!(fs::read(&fixture.linux)?, before_linux);
    assert_eq!(fs::read(&fixture.macos)?, before_macos);
    Ok(())
}

#[test]
fn expected_source_commit_is_strict_and_must_match() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    reject(&fixture, "not-a-source-sha");
    reject(&fixture, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    Ok(())
}

#[test]
fn contract_rejects_invalid_manifest_identity_inventory_and_assets() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let mut cases = Vec::new();
    let mut document = valid_document();
    document["repository"] = json!("attacker.invalid/other");
    cases.push(document);
    let mut document = valid_document();
    document["targets"][0]["artifact"] = json!("https://example.invalid/binary");
    cases.push(document);
    let mut document = valid_document();
    document["targets"][0]["sha256"] = json!("ABCDEF");
    cases.push(document);
    let mut document = valid_document();
    document["targets"][0]["target"] = json!("x86_64-pc-windows-msvc");
    cases.push(document);
    let mut document = valid_document();
    document["targets"]
        .as_array_mut()
        .ok_or("targets is an array")?
        .pop();
    cases.push(document);
    let mut document = valid_document();
    let duplicate = document["targets"][0].clone();
    document["targets"]
        .as_array_mut()
        .ok_or("targets is an array")?
        .push(duplicate);
    cases.push(document);
    let mut document = valid_document();
    document["unexpected"] = json!(true);
    cases.push(document);
    for document in &cases {
        fixture.write_document(document)?;
        reject(&fixture, SOURCE_COMMIT);
    }
    let valid_json = serde_json::to_string(&valid_document())?;
    let commit_pair = format!("\"commit\":\"{SOURCE_COMMIT}\"");
    let duplicate_pair = format!("{commit_pair},{commit_pair}");
    let duplicate_json = valid_json.replacen(&commit_pair, &duplicate_pair, 1);
    fixture.write_manifest_bytes(duplicate_json.as_bytes())?;
    let error = fixture
        .verify(SOURCE_COMMIT)
        .err()
        .ok_or("duplicate manifest key must fail")?;
    assert!(error.to_string().contains("duplicate"), "{error}");
    Ok(())
}

#[test]
fn swapped_and_mismatched_binary_paths_fail() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let result = verify_local_generator_release_manifest(
        &fixture.manifest,
        SOURCE_COMMIT,
        &fixture.macos,
        &fixture.linux,
    );
    assert!(result.is_err(), "swapped target binaries must fail");
    fs::write(&fixture.linux, b"changed\n")?;
    reject(&fixture, SOURCE_COMMIT);
    fs::write(&fixture.linux, LINUX_BYTES)?;
    fs::write(&fixture.macos, b"changed\n")?;
    let error = fixture
        .verify(SOURCE_COMMIT)
        .err()
        .ok_or("macOS binary mismatch must fail")?;
    assert!(
        error.to_string().contains("aarch64-apple-darwin"),
        "{error}"
    );
    Ok(())
}

#[test]
fn invalid_manifest_encoding_size_and_missing_paths_fail() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write_manifest_bytes(&[0xff, 0xfe])?;
    reject(&fixture, SOURCE_COMMIT);
    let missing_manifest = fixture.directory.path().join("missing-manifest.json");
    assert!(
        verify_local_generator_release_manifest(
            &missing_manifest,
            SOURCE_COMMIT,
            &fixture.linux,
            &fixture.macos,
        )
        .is_err()
    );
    let missing = fixture.directory.path().join("missing-binary");
    assert!(sha256_nonempty_file(&missing, 10).is_err());
    Ok(())
}

#[test]
fn oversized_manifest_is_rejected_after_bounded_read() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let file = fs::File::create(&fixture.manifest)?;
    file.set_len((super::MAX_MANIFEST_BYTES_U64).saturating_add(1))?;
    reject(&fixture, SOURCE_COMMIT);
    Ok(())
}

#[test]
fn empty_binary_and_binary_size_limit_fail() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fs::write(&fixture.linux, b"")?;
    reject(&fixture, SOURCE_COMMIT);
    fs::write(&fixture.linux, b"three")?;
    assert!(sha256_nonempty_file(&fixture.linux, 2).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_and_directory_inputs_are_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new()?;
    let symlink_path = fixture.directory.path().join("symlink-binary");
    symlink(&fixture.linux, &symlink_path)?;
    assert!(sha256_nonempty_file(&symlink_path, 100).is_err());
    assert!(sha256_nonempty_file(fixture.directory.path(), 100).is_err());
    Ok(())
}
