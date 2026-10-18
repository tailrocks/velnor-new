use super::super::manifest::PayloadRoot;
use super::super::roots::{CacheRootId, CacheRootSet};
use super::files::open_copy_source;
use super::payload::{
    ArchiveEntry, ArchiveEntryKind, inventory_directory, open_inventory_file,
    validate_inventory_directory, validate_inventory_symlink,
};
use super::transport_envelope::write_envelope;
use super::{FILE_DOMAIN, MAX_FILE_BYTES, root_name};
use crate::cache_action_stage::checked_stat_u32;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use tar::{Builder, EntryType, Header};

const MAX_USTAR_SIZE: u64 = 8_589_934_591;

pub(crate) fn write_payload<W: Write + Seek>(
    output: &mut W,
    operation_limit: u64,
    roots: &CacheRootSet,
    payload_roots: &[PayloadRoot],
    manifest: &str,
) -> Result<(), &'static str> {
    write_envelope(output, operation_limit, |output| {
        write_tar(output, roots, payload_roots, manifest)
    })
}

fn write_tar<W: Write>(
    output: &mut W,
    roots: &CacheRootSet,
    payload_roots: &[PayloadRoot],
    manifest: &str,
) -> Result<(), &'static str> {
    if roots.roots.len() != payload_roots.len()
        || roots
            .roots
            .iter()
            .zip(payload_roots)
            .any(|(root, payload)| root.id != payload.id)
    {
        return Err("qualification_cache_payload_roots_mismatch");
    }
    let mut archive = Builder::new(output);
    append_bytes(&mut archive, "manifest.json", 0o600, manifest.as_bytes())?;
    append_directory(&mut archive, "roots", 0o700)?;
    let mut ordered_roots = roots.roots.iter().zip(payload_roots).collect::<Vec<_>>();
    ordered_roots.sort_by_key(|(root, _)| root_name(root.id));
    for (root, expected) in ordered_roots {
        if !expected.present {
            continue;
        }
        let archive_root = format!("roots/{}", root_name(root.id));
        if is_file_root(root.id) {
            append_root_file(&mut archive, &archive_root, &root.path, expected)?;
        } else {
            append_root_directory(&mut archive, &archive_root, &root.path, expected)?;
        }
    }
    archive
        .finish()
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    Ok(())
}

fn append_root_directory<W: Write>(
    archive: &mut Builder<W>,
    archive_root: &str,
    source: &Path,
    expected: &PayloadRoot,
) -> Result<(), &'static str> {
    let Some((inventory, root)) = inventory_directory(source)? else {
        return Err("qualification_cache_payload_copy_mismatch");
    };
    if Some(&inventory.digest) != expected.digest.as_ref()
        || Some(&inventory.root_mode) != expected.mode.as_ref()
    {
        return Err("qualification_cache_payload_file_changed");
    }
    append_directory(archive, archive_root, inventory.root_mode)?;
    for entry in &inventory.entries {
        append_inventory_entry(archive, root.fd(), archive_root, entry)?;
    }
    let final_stat =
        rustix::fs::fstat(root.fd()).map_err(|_| "qualification_cache_payload_read_failed")?;
    if !super::dirs::same_version(&inventory.root_stat, &final_stat) {
        return Err("qualification_cache_payload_file_changed");
    }
    root.revalidate()?;
    Ok(())
}

fn append_inventory_entry<W: Write>(
    archive: &mut Builder<W>,
    root_fd: &rustix::fd::OwnedFd,
    archive_root: &str,
    entry: &ArchiveEntry,
) -> Result<(), &'static str> {
    let path = format!("{archive_root}/{}", entry.path);
    match entry.kind {
        ArchiveEntryKind::Directory => {
            validate_inventory_directory(root_fd, entry)?;
            append_directory(archive, &path, entry.mode)
        }
        ArchiveEntryKind::Symlink => {
            let target = validate_inventory_symlink(root_fd, entry)?;
            append_link(archive, &path, &target, entry.mode, EntryType::Symlink)
        }
        ArchiveEntryKind::File => {
            if let Some(target) = &entry.hardlink_target {
                let target = format!("{archive_root}/{target}");
                return append_link(archive, &path, &target, entry.mode, EntryType::Link);
            }
            let mut file = open_inventory_file(root_fd, entry)?;
            append_file(
                archive,
                &path,
                entry.mode,
                entry.size,
                entry.content_digest,
                &mut file,
                None,
            )?;
            let actual =
                rustix::fs::fstat(&file).map_err(|_| "qualification_cache_payload_read_failed")?;
            if !entry
                .stat
                .as_ref()
                .is_some_and(|expected| super::dirs::same_version(expected, &actual))
            {
                return Err("qualification_cache_payload_file_changed");
            }
            Ok(())
        }
    }
}

fn append_root_file<W: Write>(
    archive: &mut Builder<W>,
    archive_path: &str,
    source: &Path,
    expected: &PayloadRoot,
) -> Result<(), &'static str> {
    let (mut file, stat, source_guard) = open_copy_source(source)?;
    super::payload::validate_payload_root_mode(&stat)?;
    let size = u64::try_from(stat.st_size).map_err(|_| "qualification_cache_payload_limit")?;
    if size > MAX_FILE_BYTES {
        return Err("qualification_cache_payload_limit");
    }
    let mode = checked_stat_u32(
        stat.st_mode & 0o777,
        "qualification_cache_payload_file_changed",
    )?;
    if Some(&mode) != expected.mode.as_ref() {
        return Err("qualification_cache_payload_file_changed");
    }
    let digest = expected
        .digest
        .as_deref()
        .ok_or("qualification_cache_payload_digest_missing")?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(FILE_DOMAIN);
    hasher.update(&mode.to_be_bytes());
    hasher.update(&size.to_be_bytes());
    append_hashed_file(
        archive,
        archive_path,
        mode,
        size,
        &mut file,
        &mut hasher,
        true,
    )?;
    if format!("b3-{}", hasher.finalize().to_hex()) != digest {
        return Err("qualification_cache_payload_file_changed");
    }
    let final_stat =
        rustix::fs::fstat(&file).map_err(|_| "qualification_cache_payload_read_failed")?;
    if !super::dirs::same_version(&stat, &final_stat) {
        return Err("qualification_cache_payload_file_changed");
    }
    source_guard.revalidate(&file)?;
    Ok(())
}

fn append_file<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    mode: u32,
    size: u64,
    expected_digest: [u8; 32],
    file: &mut File,
    prefix: Option<&[u8]>,
) -> Result<(), &'static str> {
    if size > MAX_FILE_BYTES {
        return Err("qualification_cache_payload_limit");
    }
    let mut hasher = blake3::Hasher::new();
    if let Some(prefix) = prefix {
        hasher.update(prefix);
    }
    append_hashed_file(archive, path, mode, size, file, &mut hasher, false)?;
    if hasher.finalize().as_bytes() != &expected_digest {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn append_hashed_file<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    mode: u32,
    size: u64,
    file: &mut File,
    hasher: &mut blake3::Hasher,
    payload_root: bool,
) -> Result<(), &'static str> {
    if size > MAX_USTAR_SIZE {
        return Err("qualification_cache_transport_file_limit");
    }
    let header = header(path, mode, size, EntryType::Regular)?;
    let mut reader = HashReader {
        file,
        hasher,
        remaining: size,
        count: 0,
    };
    archive
        .append_pax_extensions([("path", path.as_bytes())])
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    archive
        .append(&header, &mut reader)
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    if reader.count != size || (payload_root && reader.remaining != 0) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn append_bytes<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    mode: u32,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let header = header(
        path,
        mode,
        u64::try_from(bytes.len()).map_err(|_| "qualification_cache_transport_limit_invalid")?,
        EntryType::Regular,
    )?;
    archive
        .append_pax_extensions([("path", path.as_bytes())])
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    archive
        .append(&header, bytes)
        .map_err(|_| "qualification_cache_transport_write_failed")
}

fn append_directory<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    mode: u32,
) -> Result<(), &'static str> {
    append_link(archive, path, "", mode, EntryType::Directory)
}

fn append_link<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    target: &str,
    mode: u32,
    entry_type: EntryType,
) -> Result<(), &'static str> {
    let mut header = header(path, mode, 0, entry_type)?;
    if entry_type == EntryType::Directory {
        header.set_size(0);
    }
    archive
        .append_pax_extensions(
            [("path", path.as_bytes()), ("linkpath", target.as_bytes())]
                .into_iter()
                .filter(|(_, value)| !value.is_empty()),
        )
        .map_err(|_| "qualification_cache_transport_write_failed")?;
    archive
        .append(&header, io::empty())
        .map_err(|_| "qualification_cache_transport_write_failed")
}

fn header(path: &str, mode: u32, size: u64, entry_type: EntryType) -> Result<Header, &'static str> {
    if path.is_empty() || path.len() > 4_096 || mode > 0o777 || size > MAX_USTAR_SIZE {
        return Err("qualification_cache_transport_entry_invalid");
    }
    let mut header = Header::new_ustar();
    header
        .set_path("payload-entry")
        .map_err(|_| "qualification_cache_transport_entry_invalid")?;
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(size);
    header.set_entry_type(entry_type);
    header.set_cksum();
    Ok(header)
}

fn is_file_root(id: CacheRootId) -> bool {
    matches!(
        id,
        CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml
    )
}

struct HashReader<'a> {
    file: &'a mut File,
    hasher: &'a mut blake3::Hasher,
    remaining: u64,
    count: u64,
}

impl Read for HashReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let limit =
            usize::try_from(self.remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let count = self.file.read(&mut buffer[..limit])?;
        self.hasher.update(&buffer[..count]);
        self.count += count as u64;
        self.remaining -= count as u64;
        Ok(count)
    }
}

#[cfg(test)]
#[path = "tests/dispatch_cache_transport_tar_write_tests.rs"]
mod tests;
