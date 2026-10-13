//! Bounded verification of one service-authenticated baseline ZIP.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use crate::run_select::BaselineArtifactMetadata;

/// Maximum extracted manifest bytes, aligned with baseline staging.
const MAX_BASELINE_JSON_BYTES: usize = 1_048_576;
/// One manifest plus ZIP headers and bounded central-directory metadata.
pub(crate) const MAX_BASELINE_ARCHIVE_BYTES: usize = MAX_BASELINE_JSON_BYTES + 65_536;

#[path = "baseline_archive_zip.rs"]
mod zip_admission;

/// Verify service size and SHA-256, then extract the sole `baseline.json`.
///
/// # Errors
/// Rejects oversized or mismatched bytes, malformed archives, extra entries,
/// unsafe entry types, unsupported compression, and oversized manifests.
pub(crate) fn extract_baseline(
    metadata: &BaselineArtifactMetadata,
    bytes: &[u8],
) -> Result<Vec<u8>, String> {
    let expected_size =
        usize::try_from(metadata.size_in_bytes).map_err(|_| "baseline_unavailable".to_owned())?;
    if metadata.id == 0
        || expected_size == 0
        || expected_size > MAX_BASELINE_ARCHIVE_BYTES
        || bytes.len() != expected_size
        || !valid_digest(&metadata.digest)
    {
        return Err("baseline_unavailable".to_owned());
    }
    let actual_digest = format!(
        "sha256:{}",
        crate::cover_identity::generator::sha256_hex(bytes)
    );
    if actual_digest != metadata.digest {
        return Err("baseline_unavailable".to_owned());
    }
    let shape =
        zip_admission::preflight_zip(bytes).map_err(|_| "baseline_unavailable".to_owned())?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "baseline_unavailable".to_owned())?;
    if archive.len() != 1 {
        return Err("baseline_unavailable".to_owned());
    }
    let file = archive
        .by_index(0)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    if file.name() != "baseline.json"
        || file.is_dir()
        || file.size() == 0
        || file.size() != u64::from(shape.uncompressed_size)
        || file.compressed_size() != u64::from(shape.compressed_size)
        || file.size() > MAX_BASELINE_JSON_BYTES as u64
        || !matches!(
            file.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        )
        || file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170_000 != 0o100_000)
    {
        return Err("baseline_unavailable".to_owned());
    }
    if shape.method == 8 {
        let start = usize::try_from(
            file.data_start()
                .ok_or_else(|| "baseline_unavailable".to_owned())?,
        )
        .map_err(|_| "baseline_unavailable".to_owned())?;
        let end = start
            .checked_add(
                usize::try_from(shape.compressed_size)
                    .map_err(|_| "baseline_unavailable".to_owned())?,
            )
            .ok_or_else(|| "baseline_unavailable".to_owned())?;
        let compressed = bytes
            .get(start..end)
            .ok_or_else(|| "baseline_unavailable".to_owned())?;
        if !zip_admission::exact_deflate_consumption(compressed, shape.uncompressed_size) {
            return Err("baseline_unavailable".to_owned());
        }
    }
    let declared_size =
        usize::try_from(file.size()).map_err(|_| "baseline_unavailable".to_owned())?;
    read_manifest(file, declared_size)
}

/// Cap actual decompressed bytes before growing the output vector.
fn read_manifest(reader: impl Read, declared_size: usize) -> Result<Vec<u8>, String> {
    let limit = u64::try_from(MAX_BASELINE_JSON_BYTES + 1)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let mut manifest = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut manifest)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    if manifest.len() != declared_size || manifest.len() > MAX_BASELINE_JSON_BYTES {
        return Err("baseline_unavailable".to_owned());
    }
    Ok(manifest)
}

/// Verify and stage one archive beneath its validated artifact-name directory.
/// # Errors
/// Returns a miss if metadata, ZIP contents, or exclusive staging fails.
pub(crate) fn stage_baseline_archive(
    staging_root: &Path,
    artifact_name: &str,
    metadata: &BaselineArtifactMetadata,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    velnor_actions_contract::validate_artifact_id(artifact_name)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let manifest = extract_baseline(metadata, bytes)?;
    let entry = staging_root.join(artifact_name);
    std::fs::create_dir(&entry).map_err(|_| "baseline_unavailable".to_owned())?;
    let path = entry.join("baseline.json");
    crate::exclusive_write::write_exclusive(&path, &manifest, "baseline_zip")
        .map_err(|_| "baseline_unavailable".to_owned())?;
    Ok(path)
}

/// GitHub artifact digest is lowercase `sha256:` plus 64 hex digits.
fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Test-only archive builder shared by baseline acquisition regressions.
#[cfg(test)]
pub(crate) fn test_archive(name: &str, payload: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut writer = zip::ZipWriter::new_stream(Vec::new());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(name, options).expect("start ZIP member");
    writer.write_all(payload).expect("write ZIP member");
    writer.finish().expect("finish ZIP archive").into_inner()
}

#[cfg(test)]
#[path = "baseline_archive_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "baseline_archive_tests_b.rs"]
mod tests_b;
