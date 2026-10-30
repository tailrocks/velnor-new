use super::super::super::manifest::{PayloadManifest, PayloadRoot};
use super::super::super::roots::{CacheRootId, CacheRootSet};
use super::super::payload::{self, ArchiveEntry, ArchiveEntryKind};
use super::super::transport_envelope::RawTarRange;
use super::{
    FILE_DOMAIN, HeaderFields, MAX_ENTRIES, MAX_INVENTORY_TEXT_BYTES, MemberKind, TransportEntry,
    ValidatedTransport, effective_value, hash_entry, hex, is_file_root, root_name, utf8_text,
    validate_archive_path,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use tar::Entry;

pub(super) struct ParseState<'a> {
    roots: BTreeMap<CacheRootId, RootState<'a>>,
    entries: Vec<TransportEntry>,
    seen: BTreeSet<String>,
    inventory_text_bytes: usize,
    previous_files: BTreeMap<(CacheRootId, String), FileIdentity>,
    count: usize,
}

struct RootState<'a> {
    manifest: &'a PayloadRoot,
    root_seen: bool,
    root_mode: Option<u32>,
    entries: Vec<ArchiveEntry>,
}

#[derive(Clone)]
struct FileIdentity {
    mode: u32,
    size: u64,
    digest: [u8; 32],
    hardlink_target: Option<String>,
}

impl<'a> ParseState<'a> {
    pub(super) fn new(manifest: &'a PayloadManifest) -> Self {
        Self {
            roots: manifest
                .roots
                .iter()
                .map(|root| {
                    (
                        root.id,
                        RootState {
                            manifest: root,
                            root_seen: false,
                            root_mode: None,
                            entries: Vec::new(),
                        },
                    )
                })
                .collect(),
            entries: Vec::new(),
            seen: BTreeSet::new(),
            inventory_text_bytes: 0,
            previous_files: BTreeMap::new(),
            count: 2,
        }
    }

    pub(super) fn add_member(&mut self, path: &str) -> Result<(), &'static str> {
        self.count = self
            .count
            .checked_add(1)
            .filter(|count| *count <= MAX_ENTRIES)
            .ok_or("qualification_cache_payload_limit")?;
        self.inventory_text_bytes = self
            .inventory_text_bytes
            .checked_add(path.len())
            .filter(|size| *size <= MAX_INVENTORY_TEXT_BYTES)
            .ok_or("qualification_cache_payload_limit")?;
        if !self.seen.insert(path.to_owned()) {
            return Err("qualification_cache_payload_path_duplicate");
        }
        Ok(())
    }

    pub(super) fn read_member<R: Read>(
        &mut self,
        entry: &mut Entry<'_, R>,
        archive_path: &str,
        fields: HeaderFields,
        data_offset: Option<u64>,
    ) -> Result<(), &'static str> {
        let (root_id, relative, root_member) = self.classify_path(archive_path)?;
        if !root_member && !self.has_parent_directories(root_id, &relative) {
            return Err("qualification_cache_payload_parent_missing");
        }
        let root = self
            .roots
            .get_mut(&root_id)
            .ok_or("qualification_cache_payload_root_mismatch")?;
        if !root.manifest.present {
            return Err("qualification_cache_payload_root_mismatch");
        }
        let expected_file_root = is_file_root(root_id);
        if root_member {
            if root.root_seen {
                return Err("qualification_cache_payload_path_duplicate");
            }
            root.root_seen = true;
            if expected_file_root {
                if !fields.kind.is_file() {
                    return Err("qualification_cache_payload_root_kind_mismatch");
                }
            } else if !fields.kind.is_dir() || fields.size != 0 {
                return Err("qualification_cache_payload_root_kind_mismatch");
            }
            if root.manifest.mode != Some(fields.mode) {
                return Err("qualification_cache_payload_root_mode_mismatch");
            }
            root.root_mode = Some(fields.mode);
        } else {
            if expected_file_root || !root.root_seen {
                return Err("qualification_cache_payload_root_mismatch");
            }
        }

        let (kind, size, digest, symlink_target, hardlink_target) = match fields.kind {
            kind if kind.is_file() => {
                let prefix = if root_member {
                    let mut prefix = FILE_DOMAIN.to_vec();
                    prefix.extend_from_slice(&fields.mode.to_be_bytes());
                    prefix.extend_from_slice(&fields.size.to_be_bytes());
                    Some(prefix)
                } else {
                    None
                };
                let (digest, read_size) = hash_entry(entry, fields.size, prefix.as_deref())?;
                if root_member {
                    let expected = format!("b3-{}", hex(&digest));
                    if root.manifest.digest.as_deref() != Some(expected.as_str()) {
                        return Err("qualification_cache_payload_digest_mismatch");
                    }
                } else {
                    self.previous_files.insert(
                        (root_id, relative.clone()),
                        FileIdentity {
                            mode: fields.mode,
                            size: fields.size,
                            digest,
                            hardlink_target: None,
                        },
                    );
                }
                (MemberKind::File, read_size, digest, None, None)
            }
            kind if kind.is_dir() => {
                if fields.size != 0 {
                    return Err("qualification_cache_payload_entry_invalid");
                }
                if !root_member {
                    self.seen
                        .insert(format!("roots/{}/{}", root_name(root_id), relative));
                }
                (MemberKind::Directory, 0, [0; 32], None, None)
            }
            kind if kind.is_symlink() => {
                if fields.size != 0 || root_member {
                    return Err("qualification_cache_payload_entry_invalid");
                }
                let target = effective_value(entry, "linkpath")?;
                let target = utf8_text(target, "qualification_cache_payload_symlink_invalid")?;
                payload::validate_internal_link(&relative, &target)?;
                self.inventory_text_bytes = self
                    .inventory_text_bytes
                    .checked_add(target.len())
                    .filter(|size| *size <= MAX_INVENTORY_TEXT_BYTES)
                    .ok_or("qualification_cache_payload_limit")?;
                let size =
                    u64::try_from(target.len()).map_err(|_| "qualification_cache_payload_limit")?;
                let digest = *blake3::hash(target.as_bytes()).as_bytes();
                (MemberKind::Symlink, size, digest, Some(target), None)
            }
            kind if kind.is_hard_link() => {
                if fields.size != 0 || root_member {
                    return Err("qualification_cache_payload_entry_invalid");
                }
                let target = effective_value(entry, "linkpath")?;
                let target = utf8_text(target, "qualification_cache_payload_hardlink_invalid")?;
                let target_prefix = format!("roots/{}/", root_name(root_id));
                let target = target
                    .strip_prefix(&target_prefix)
                    .ok_or("qualification_cache_payload_hardlink_invalid")?
                    .to_owned();
                validate_archive_path(&target)?;
                self.inventory_text_bytes = self
                    .inventory_text_bytes
                    .checked_add(target.len())
                    .filter(|size| *size <= MAX_INVENTORY_TEXT_BYTES)
                    .ok_or("qualification_cache_payload_limit")?;
                let previous = self
                    .previous_files
                    .get(&(root_id, target.clone()))
                    .filter(|file| file.hardlink_target.is_none())
                    .ok_or("qualification_cache_payload_hardlink_invalid")?;
                if previous.mode != fields.mode {
                    return Err("qualification_cache_payload_hardlink_invalid");
                }
                let identity = previous.clone();
                self.previous_files.insert(
                    (root_id, relative.clone()),
                    FileIdentity {
                        hardlink_target: Some(target.clone()),
                        ..identity.clone()
                    },
                );
                (
                    MemberKind::File,
                    identity.size,
                    identity.digest,
                    None,
                    Some(target),
                )
            }
            _ => return Err("qualification_cache_payload_entry_unsupported"),
        };
        if !root_member {
            self.inventory_text_bytes = self
                .inventory_text_bytes
                .checked_add(relative.len())
                .filter(|size| *size <= MAX_INVENTORY_TEXT_BYTES)
                .ok_or("qualification_cache_payload_limit")?;
            let entry = TransportEntry {
                root: root_id,
                path: relative.clone(),
                kind,
                mode: fields.mode,
                size,
                data_offset,
                digest,
                symlink_target: symlink_target.clone(),
                hardlink_target: hardlink_target.clone(),
            };
            root.entries.push(ArchiveEntry {
                path: relative,
                kind: match kind {
                    MemberKind::Directory => ArchiveEntryKind::Directory,
                    MemberKind::File => ArchiveEntryKind::File,
                    MemberKind::Symlink => ArchiveEntryKind::Symlink,
                },
                mode: fields.mode,
                size,
                content_digest: digest,
                hardlink_target,
                link_target: symlink_target,
                stat: None,
            });
            self.entries.push(entry);
        } else {
            self.entries.push(TransportEntry {
                root: root_id,
                path: String::new(),
                kind,
                mode: fields.mode,
                size,
                data_offset,
                digest,
                symlink_target,
                hardlink_target,
            });
        }
        Ok(())
    }

    fn classify_path(&self, path: &str) -> Result<(CacheRootId, String, bool), &'static str> {
        let suffix = path
            .strip_prefix("roots/")
            .ok_or("qualification_cache_payload_path_invalid")?;
        let (root_name_text, relative) = suffix
            .split_once('/')
            .map_or((suffix, None), |(root, relative)| (root, Some(relative)));
        let root_id = self
            .roots
            .keys()
            .copied()
            .find(|root| root_name(*root) == root_name_text)
            .ok_or("qualification_cache_payload_root_mismatch")?;
        let Some(relative) = relative else {
            return Ok((root_id, String::new(), true));
        };
        validate_archive_path(relative)?;
        Ok((root_id, relative.to_owned(), false))
    }

    fn has_parent_directories(&self, root_id: CacheRootId, relative: &str) -> bool {
        let mut parts = relative.split('/').peekable();
        let mut parent = String::new();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if parent.is_empty() {
                parent.push_str(part);
            } else {
                parent.push('/');
                parent.push_str(part);
            }
            if !self
                .seen
                .contains(&format!("roots/{}/{parent}", root_name(root_id)))
            {
                return false;
            }
        }
        true
    }

    pub(super) fn finish(
        self,
        manifest: PayloadManifest,
        range: RawTarRange,
        roots: &CacheRootSet,
    ) -> Result<ValidatedTransport, &'static str> {
        if self.roots.len() != roots.roots.len() {
            return Err("qualification_cache_payload_root_mismatch");
        }
        let mut root_modes = BTreeMap::new();
        for root in &manifest.roots {
            let state = self
                .roots
                .get(&root.id)
                .ok_or("qualification_cache_payload_root_mismatch")?;
            if root.present != state.root_seen {
                return Err("qualification_cache_payload_root_mismatch");
            }
            if !root.present {
                continue;
            }
            let mode = state
                .root_mode
                .ok_or("qualification_cache_payload_root_mismatch")?;
            if is_file_root(root.id) {
                if !state.entries.is_empty() {
                    return Err("qualification_cache_payload_root_mismatch");
                }
            } else {
                let digest = payload::digest_archive_entries(mode, &state.entries)?;
                if root.digest.as_deref() != Some(digest.as_str()) {
                    return Err("qualification_cache_payload_digest_mismatch");
                }
            }
            root_modes.insert(root.id, mode);
        }
        if super::super::useful_state_digest(&manifest.roots)? != manifest.state_digest {
            return Err("qualification_cache_payload_state_mismatch");
        }
        Ok(ValidatedTransport {
            manifest,
            range,
            entries: self.entries,
            root_modes,
        })
    }
}
