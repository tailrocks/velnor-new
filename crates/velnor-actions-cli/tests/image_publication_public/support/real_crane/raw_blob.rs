// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use tempfile::tempdir;
use velnor_actions_mise::{CheckDeadline, IsolatedCommand, ProcessFileOutput};

use super::super::oci;
use super::{
    assert_published_bytes, crane, crane_command, path_text, pinned_crane_version,
    published_blob_inventory, registry::Registry,
};

const STDERR_LIMIT: usize = 1024 * 1024;
const COMPARE_CHUNK_BYTES: usize = 16 * 1024;

#[test]
fn pinned_crane_raw_blob_readback_preserves_legacy_job_images() -> Result<(), Box<dyn Error>> {
    let version = pinned_crane_version()?;
    eprintln!("raw Crane blob compatibility uses pinned version {version}");
    let source = "c".repeat(40);
    let cases = vec![
        ("normal", oci::create(&source)?),
        (
            "missing-revision",
            oci::create_with_image_config(&serde_json::json!({"User":"fixture"}))?,
        ),
        (
            "Cmd17",
            oci::create_with_image_config(&serde_json::json!({"Cmd":17}))?,
        ),
        (
            "raw-tar",
            oci::create_with_raw_tar(&serde_json::json!({"User":"fixture"}))?,
        ),
    ];
    for (tag, layout) in cases {
        let temp = tempdir()?;
        let directory = temp.path().join("layout");
        layout.write_directory(&directory)?;
        let registry = Registry::new()?;
        let reference = registry.reference(tag);
        let push = crane(&["push", "--insecure", path_text(&directory)?, &reference])?;
        assert!(
            push.success,
            "crane push {tag} failed: {}",
            String::from_utf8_lossy(&push.stderr)
        );
        let digest = layout.identity.platform_manifest_digest.clone();
        assert_published_bytes(&registry, &layout, &digest)?;
        let readback = temp.path().join("readback");
        fs::create_dir(&readback)?;
        assert_remote_blobs(&registry, &layout, &reference, &readback)?;
        if tag == "raw-tar" {
            assert_raw_tar_manifest(&layout)?;
        }
        eprintln!(
            "raw Crane blob case {tag}: manifest={digest} config={}",
            layout.identity.image_configuration_digest
        );
        assert_eq!(
            registry.stored_inventory()?,
            published_blob_inventory(&layout)
        );
        registry.require_healthy()?;
    }
    Ok(())
}

#[test]
fn pinned_crane_raw_blob_acceptance_rejects_corrupt_config_and_layer() -> Result<(), Box<dyn Error>>
{
    let version = pinned_crane_version()?;
    eprintln!("raw Crane corruption probe uses pinned version {version}");
    for (tag, kind) in [("bad-config", "config"), ("bad-layer", "layer")] {
        let temp = tempdir()?;
        let layout = oci::create("d".repeat(40).as_str())?;
        let directory = temp.path().join("layout");
        layout.write_directory(&directory)?;
        let registry = Registry::new()?;
        let reference = registry.reference(tag);
        let push = crane(&["push", "--insecure", path_text(&directory)?, &reference])?;
        assert!(push.success, "crane push {tag} failed");
        let digest = layout.identity.platform_manifest_digest.clone();
        assert_published_bytes(&registry, &layout, &digest)?;
        let blob_digest = match kind {
            "config" => &layout.identity.image_configuration_digest,
            _ => layout
                .identity
                .layer_digests
                .first()
                .ok_or("layout has no layer")?,
        };
        let before = registry.stored_inventory()?;
        registry.corrupt_blob(blob_digest)?;
        let after = registry.stored_inventory()?;
        let expected = layout
            .inventory
            .get(&blob_path(blob_digest))
            .ok_or("fixture blob bytes are missing")?;
        let corrupted = after
            .get(&blob_path(blob_digest))
            .ok_or("corrupted fixture blob bytes are missing")?;
        assert_eq!(corrupted.len(), expected.len());
        assert_ne!(corrupted, expected);
        assert_ne!(after, before);
        let output_path = temp.path().join("corrupted-blob");
        let (output, matches) = read_remote_blob(&registry, blob_digest, expected, &output_path)?;
        let accepted = output.success && output.stdout_bytes == expected.len() as u64 && matches;
        assert!(!accepted, "raw blob acceptance accepted corrupt {kind}");
        eprintln!(
            "raw Crane corrupted {kind}: success={} exit={:?} bytes={} exact_match={matches} stderr={:?}",
            output.success,
            output.code,
            output.stdout_bytes,
            String::from_utf8_lossy(&output.stderr)
        );
        registry.require_healthy()?;
    }
    Ok(())
}

fn assert_remote_blobs(
    registry: &Registry,
    layout: &oci::Layout,
    reference: &str,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    let manifest_path = blob_path(&layout.identity.platform_manifest_digest);
    for (path, expected) in &layout.inventory {
        let name = Path::new(path)
            .file_name()
            .ok_or("fixture blob path has no name")?
            .to_string_lossy();
        let digest = format!("sha256:{name}");
        let output_path = destination.join(name.as_ref());
        let (output, matches) = if *path == manifest_path {
            read_remote_manifest(reference, expected, &output_path)?
        } else {
            read_remote_blob(registry, &digest, expected, &output_path)?
        };
        assert!(
            output.success,
            "crane blob {digest} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout_bytes, expected.len() as u64);
        assert!(matches, "remote blob {digest} differs from admitted bytes");
        eprintln!(
            "raw Crane readback digest={digest} bytes={} exact_match={matches}",
            output.stdout_bytes
        );
    }
    Ok(())
}

fn read_remote_blob(
    registry: &Registry,
    digest: &str,
    expected: &[u8],
    destination: &Path,
) -> Result<(ProcessFileOutput, bool), Box<dyn Error>> {
    let reference = registry.digest_reference(digest);
    let command = crane_command(&["blob", "--insecure", &reference])?;
    capture_command(&command, expected, destination)
}

fn read_remote_manifest(
    reference: &str,
    expected: &[u8],
    destination: &Path,
) -> Result<(ProcessFileOutput, bool), Box<dyn Error>> {
    let command = crane_command(&["manifest", "--insecure", reference])?;
    capture_command(&command, expected, destination)
}

fn capture_command(
    command: &IsolatedCommand,
    expected: &[u8],
    destination: &Path,
) -> Result<(ProcessFileOutput, bool), Box<dyn Error>> {
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = command.run_stdout_to_file_until(
        &mut output,
        expected.len() as u64,
        STDERR_LIMIT,
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let matches = file_matches(&mut output, expected)?;
    Ok((result, matches))
}

fn file_matches(file: &mut File, expected: &[u8]) -> std::io::Result<bool> {
    file.seek(SeekFrom::Start(0))?;
    let mut offset = 0;
    let mut chunk = [0; COMPARE_CHUNK_BYTES];
    while offset < expected.len() {
        let limit = chunk.len().min(expected.len() - offset);
        let count = file.read(&mut chunk[..limit])?;
        if count == 0 || chunk[..count] != expected[offset..offset + count] {
            return Ok(false);
        }
        offset += count;
    }
    let mut extra = [0; 1];
    Ok(file.read(&mut extra)? == 0)
}

fn blob_path(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

fn assert_raw_tar_manifest(layout: &oci::Layout) -> Result<(), Box<dyn Error>> {
    let manifest = layout
        .inventory
        .get(&blob_path(&layout.identity.platform_manifest_digest))
        .ok_or("raw fixture manifest is missing")?;
    let manifest: serde_json::Value = serde_json::from_slice(manifest)?;
    assert_eq!(
        manifest["layers"][0]["mediaType"],
        "application/vnd.oci.image.layer.v1.tar"
    );
    Ok(())
}
