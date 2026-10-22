//! Validate cache tar members and bind them to the format-3 manifest.

use super::super::manifest::PayloadManifest;
use super::super::roots::{CacheRootId, CacheRootSet};
use super::transport_envelope::read_envelope;
#[path = "transfer_transport_tar_read_state.rs"]
mod state;
use super::{FILE_DOMAIN, root_name};
use state::ParseState;
use std::io::{Read, Seek, SeekFrom};
use tar::{Archive, Entry, EntryType};

const MAX_ENTRIES: usize = 250_000;
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_INVENTORY_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::qualification_cache_payload) enum MemberKind {
    Directory,
    File,
    Symlink,
}

/// Check framing, manifest binding, every effective path and every root digest.
pub(in crate::qualification_cache_payload) fn preflight<R, F>(
    reader: &mut R,
    operation_limit: u64,
    roots: &CacheRootSet,
    parse_manifest: F,
) -> Result<PayloadManifest, &'static str>
where
    R: Read + Seek,
    F: FnOnce(&str) -> Result<PayloadManifest, &'static str>,
{
    let range = read_envelope(reader, operation_limit)?;
    reader
        .seek(SeekFrom::Start(range.offset))
        .map_err(|_| "qualification_cache_transport_read_failed")?;
    let limited = (&mut *reader).take(range.length);
    let mut archive = Archive::new(limited);
    let mut entries = archive
        .entries()
        .map_err(|_| "qualification_cache_transport_tar_invalid")?;
    let manifest_entry = entries
        .next()
        .ok_or("qualification_cache_manifest_missing")?
        .map_err(|_| "qualification_cache_transport_tar_invalid")?;
    let manifest_text = read_manifest(manifest_entry)?;
    let manifest = parse_manifest(&manifest_text)?;
    validate_manifest_roots(&manifest, roots)?;

    let roots_entry = entries
        .next()
        .ok_or("qualification_cache_payload_layout_invalid")?
        .map_err(|_| "qualification_cache_transport_tar_invalid")?;
    validate_wrapper_directory(roots_entry, "roots", 0o700)?;

    let mut state = ParseState::new(&manifest);
    let mut previous_path = "roots".to_owned();
    for item in entries {
        let mut entry = item.map_err(|_| "qualification_cache_transport_tar_invalid")?;
        let path = effective_value(&mut entry, "path")?;
        let path = utf8_text(path, "qualification_cache_payload_path_invalid")?;
        validate_archive_path(&path)?;
        if path <= previous_path {
            return Err("qualification_cache_payload_path_order_invalid");
        }
        previous_path = path.clone();
        state.add_member(&path)?;
        let fields = canonical_header(&entry)?;
        state.read_member(&mut entry, &path, fields)?;
    }
    state.finish(manifest.clone(), roots)
}

struct HeaderFields {
    kind: EntryType,
    mode: u32,
    size: u64,
}

fn read_manifest<R: Read>(mut entry: Entry<'_, R>) -> Result<String, &'static str> {
    let path = effective_value(&mut entry, "path")?;
    if path != b"manifest.json" {
        return Err("qualification_cache_payload_layout_invalid");
    }
    let fields = canonical_header(&entry)?;
    if !fields.kind.is_file() || fields.mode != 0o600 || fields.size > MAX_MANIFEST_BYTES as u64 {
        return Err("qualification_cache_manifest_invalid");
    }
    let mut bytes = Vec::with_capacity(usize::try_from(fields.size).unwrap_or_default());
    (&mut entry)
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "qualification_cache_manifest_invalid")?;
    if u64::try_from(bytes.len()).ok() != Some(fields.size) || bytes.len() > MAX_MANIFEST_BYTES {
        return Err("qualification_cache_manifest_invalid");
    }
    String::from_utf8(bytes).map_err(|_| "qualification_cache_manifest_invalid")
}

fn validate_wrapper_directory<R: Read>(
    mut entry: Entry<'_, R>,
    expected_path: &str,
    expected_mode: u32,
) -> Result<(), &'static str> {
    if effective_value(&mut entry, "path")? != expected_path.as_bytes() {
        return Err("qualification_cache_payload_layout_invalid");
    }
    let fields = canonical_header(&entry)?;
    if !fields.kind.is_dir() || fields.mode != expected_mode || fields.size != 0 {
        return Err("qualification_cache_payload_layout_invalid");
    }
    Ok(())
}

fn canonical_header<R: Read>(entry: &Entry<'_, R>) -> Result<HeaderFields, &'static str> {
    let header = entry.header();
    let mode = header
        .mode()
        .map_err(|_| "qualification_cache_transport_header_invalid")?;
    let size = entry.size();
    if header
        .uid()
        .map_err(|_| "qualification_cache_transport_header_invalid")?
        != 0
        || header
            .gid()
            .map_err(|_| "qualification_cache_transport_header_invalid")?
            != 0
        || header
            .mtime()
            .map_err(|_| "qualification_cache_transport_header_invalid")?
            != 0
        || mode & 0o7000 != 0
        || mode > 0o777
        || size > MAX_ENTRY_BYTES
    {
        return Err("qualification_cache_transport_header_invalid");
    }
    Ok(HeaderFields {
        kind: header.entry_type(),
        mode,
        size,
    })
}

fn effective_value<R: Read>(
    entry: &mut Entry<'_, R>,
    expected_key: &str,
) -> Result<Vec<u8>, &'static str> {
    let mut found = None;
    if let Some(extensions) = entry
        .pax_extensions()
        .map_err(|_| "qualification_cache_transport_header_invalid")?
    {
        for extension in extensions {
            let extension =
                extension.map_err(|_| "qualification_cache_transport_header_invalid")?;
            if extension.key().ok() == Some(expected_key) {
                if found.is_some() {
                    return Err("qualification_cache_transport_header_invalid");
                }
                found = Some(extension.value_bytes().to_vec());
            }
        }
    }
    found.ok_or("qualification_cache_transport_header_invalid")
}

fn utf8_text(bytes: Vec<u8>, error: &'static str) -> Result<String, &'static str> {
    String::from_utf8(bytes).map_err(|_| error)
}

fn validate_archive_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || byte == b'\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("qualification_cache_payload_path_invalid");
    }
    Ok(())
}

fn validate_manifest_roots(
    manifest: &PayloadManifest,
    roots: &CacheRootSet,
) -> Result<(), &'static str> {
    if manifest.roots.len() != roots.roots.len()
        || manifest
            .roots
            .iter()
            .zip(&roots.roots)
            .any(|(expected, observed)| expected.id != observed.id)
    {
        return Err("qualification_cache_payload_root_mismatch");
    }
    Ok(())
}

fn hash_entry<R: Read>(
    entry: &mut Entry<'_, R>,
    size: u64,
    prefix: Option<&[u8]>,
) -> Result<([u8; 32], u64), &'static str> {
    let mut hasher = blake3::Hasher::new();
    if let Some(prefix) = prefix {
        hasher.update(prefix);
    }
    let mut read = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    while read < size {
        let cap = usize::try_from((size - read).min(buffer.len() as u64)).unwrap_or(buffer.len());
        let amount = entry
            .read(&mut buffer[..cap])
            .map_err(|_| "qualification_cache_payload_read_failed")?;
        if amount == 0 {
            return Err("qualification_cache_transport_entry_truncated");
        }
        read = read
            .checked_add(u64::try_from(amount).map_err(|_| "qualification_cache_payload_limit")?)
            .ok_or("qualification_cache_payload_limit")?;
        hasher.update(&buffer[..amount]);
    }
    let mut extra = [0_u8; 1];
    if entry
        .read(&mut extra)
        .map_err(|_| "qualification_cache_payload_read_failed")?
        != 0
    {
        return Err("qualification_cache_transport_entry_overflow");
    }
    Ok((*hasher.finalize().as_bytes(), read))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn is_file_root(id: CacheRootId) -> bool {
    matches!(
        id,
        CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml
    )
}
