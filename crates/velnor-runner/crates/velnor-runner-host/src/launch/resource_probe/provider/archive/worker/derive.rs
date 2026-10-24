use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::Value;
use tar::{Archive, Builder, EntryType, Header};

use super::ProviderFailure;
use super::archive_read::{hash_archive, read_bounded_entry, safe_path};
use super::layout::valid_docker_reference;
use super::model::{DerivedLoadArchive, ParsedWorkerArchive, WorkerImage};
use super::valid_sha;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub(in crate::launch::resource_probe::provider) fn derive_load_archive(
    source_path: &Path,
    output_path: &Path,
    image: WorkerImage,
    source_sha: &str,
    original: &ParsedWorkerArchive,
) -> Result<DerivedLoadArchive, ProviderFailure> {
    if !valid_sha(source_sha)
        || source_path == output_path
        || !output_path.is_absolute()
        || output_path.exists()
        || hash_archive(source_path)? != original.archive_sha256
    {
        return Err(ProviderFailure::Archive);
    }
    let tag = format!("v{source_sha}-{}", original.archive_sha256);
    let repo_tag = format!("{}:{tag}", image.image_name());
    if tag.len() > 128 || !valid_docker_reference(image, &repo_tag) {
        return Err(ProviderFailure::Archive);
    }
    let result = (|| {
        write_derived_archive(source_path, output_path, image, &repo_tag, &tag)?;
        let sha256 = hash_archive(output_path)?;
        let parsed = super::verify::verify(output_path, image, source_sha, &sha256, &repo_tag)?;
        if !original.same_image_payload(&parsed) || parsed.repo_tag != repo_tag {
            return Err(ProviderFailure::Archive);
        }
        Ok(DerivedLoadArchive {
            path: output_path.to_path_buf(),
            sha256,
            archive_index_digest: parsed.archive_index_digest.clone(),
            repo_tag: repo_tag.clone(),
            parsed,
        })
    })();
    if result.is_err() {
        drop(fs::remove_file(output_path));
    }
    result
}

pub(super) fn write_derived_archive(
    source_path: &Path,
    output_path: &Path,
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<(), ProviderFailure> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output_path)
        .map_err(|_| ProviderFailure::Archive)?;
    copy_archive_with_owned_reference(source_path, output, image, repo_tag, ref_name)
}

pub(super) fn copy_archive_with_owned_reference(
    source_path: &Path,
    output: File,
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<(), ProviderFailure> {
    let source = File::open(source_path).map_err(|_| ProviderFailure::Archive)?;
    let mut archive = Archive::new(source);
    let mut builder = Builder::new(output);
    for entry in archive.entries().map_err(|_| ProviderFailure::Archive)? {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        let raw_path = entry
            .path()
            .map_err(|_| ProviderFailure::Archive)?
            .to_str()
            .ok_or(ProviderFailure::Archive)?
            .to_owned();
        let kind = entry.header().entry_type();
        let path = if kind == EntryType::Directory {
            raw_path
                .strip_suffix('/')
                .ok_or(ProviderFailure::Archive)?
                .to_owned()
        } else {
            raw_path
        };
        if !safe_path(&path) {
            return Err(ProviderFailure::Archive);
        }
        match kind {
            EntryType::Directory => {
                let header = entry.header().clone();
                builder
                    .append(&header, std::io::empty())
                    .map_err(|_| ProviderFailure::Archive)?;
            }
            EntryType::Regular if path == "index.json" => {
                let size = entry.size();
                let bytes = read_bounded_entry(&mut entry, size, 64 * 1024)?;
                let value = rewrite_outer_index(&bytes, image, repo_tag, ref_name)?;
                append_rewritten(&mut builder, entry.header(), &value)?;
            }
            EntryType::Regular if path == "manifest.json" => {
                let size = entry.size();
                let bytes = read_bounded_entry(&mut entry, size, 256 * 1024)?;
                let value = rewrite_legacy_manifest(&bytes, repo_tag)?;
                append_rewritten(&mut builder, entry.header(), &value)?;
            }
            EntryType::Regular => {
                let header = entry.header().clone();
                builder
                    .append(&header, &mut entry)
                    .map_err(|_| ProviderFailure::Archive)?;
            }
            _ => return Err(ProviderFailure::Archive),
        }
    }
    let mut output = builder.into_inner().map_err(|_| ProviderFailure::Archive)?;
    output.flush().map_err(|_| ProviderFailure::Archive)?;
    output.sync_all().map_err(|_| ProviderFailure::Archive)
}

pub(super) fn rewrite_outer_index(
    bytes: &[u8],
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<Vec<u8>, ProviderFailure> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let descriptor = value
        .pointer_mut("/manifests/0")
        .and_then(Value::as_object_mut)
        .ok_or(ProviderFailure::Archive)?;
    if descriptor
        .get("annotations")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err(ProviderFailure::Archive);
    }
    let annotations = descriptor
        .get_mut("annotations")
        .and_then(Value::as_object_mut)
        .ok_or(ProviderFailure::Archive)?;
    let expected_tag = image.release_tag();
    let expected_ref_name = expected_tag
        .split_once(':')
        .map(|(_, tag)| tag)
        .ok_or(ProviderFailure::Archive)?;
    if annotations
        .get("io.containerd.image.name")
        .and_then(Value::as_str)
        != Some(format!("{}:{expected_ref_name}", image.docker_image_name()).as_str())
        || annotations
            .get("org.opencontainers.image.ref.name")
            .and_then(Value::as_str)
            != Some(expected_ref_name)
    {
        return Err(ProviderFailure::Archive);
    }
    let (name, tag) = repo_tag.split_once(':').ok_or(ProviderFailure::Archive)?;
    if name != image.image_name() || tag != ref_name || !valid_docker_reference(image, repo_tag) {
        return Err(ProviderFailure::Archive);
    }
    annotations.insert(
        "io.containerd.image.name".to_owned(),
        Value::String(format!("{}:{tag}", image.docker_image_name())),
    );
    annotations.insert(
        "org.opencontainers.image.ref.name".to_owned(),
        Value::String(ref_name.to_owned()),
    );
    serde_json::to_vec(&value).map_err(|_| ProviderFailure::Archive)
}

pub(super) fn rewrite_legacy_manifest(
    bytes: &[u8],
    repo_tag: &str,
) -> Result<Vec<u8>, ProviderFailure> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let entries = value.as_array_mut().ok_or(ProviderFailure::Archive)?;
    if entries.len() != 1 {
        return Err(ProviderFailure::Archive);
    }
    let manifest = entries[0].as_object_mut().ok_or(ProviderFailure::Archive)?;
    manifest.insert(
        "RepoTags".to_owned(),
        Value::Array(vec![Value::String(repo_tag.to_owned())]),
    );
    serde_json::to_vec(&value).map_err(|_| ProviderFailure::Archive)
}

pub(super) fn append_rewritten<W: Write>(
    builder: &mut Builder<W>,
    original: &Header,
    bytes: &[u8],
) -> Result<(), ProviderFailure> {
    let size = u64::try_from(bytes.len()).map_err(|_| ProviderFailure::Archive)?;
    let mut header = original.clone();
    header.set_size(size);
    header.set_cksum();
    builder
        .append(&header, bytes)
        .map_err(|_| ProviderFailure::Archive)
}
