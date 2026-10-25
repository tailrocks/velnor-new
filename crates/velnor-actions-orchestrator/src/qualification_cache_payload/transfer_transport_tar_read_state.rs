use super::super::super::manifest::{PayloadManifest, PayloadRoot};
use super::super::super::roots::{CacheRootId, CacheRootSet};
use super::super::payload::{self, ArchiveEntry, ArchiveEntryKind};
use super::{
    HeaderFields, MAX_ENTRIES, MAX_INVENTORY_TEXT_BYTES, MemberKind, is_file_root, root_name,
    validate_archive_path,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use tar::Entry;

#[path = "transfer_transport_tar_read_state/member.rs"]
mod member;

pub(super) struct ParseState<'a> {
    roots: BTreeMap<CacheRootId, RootState<'a>>,
    seen: BTreeSet<String>,
    directories: BTreeSet<String>,
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

struct MemberContent {
    kind: MemberKind,
    size: u64,
    digest: [u8; 32],
    link_target: Option<String>,
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
            seen: BTreeSet::new(),
            directories: BTreeSet::new(),
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
    ) -> Result<(), &'static str> {
        let (root_id, relative, root_member) = self.classify_path(archive_path)?;
        self.validate_member_parent(root_id, &relative, root_member)?;
        self.validate_root_member(root_id, root_member, fields)?;
        let content = member::read_member_content(
            self,
            entry,
            root_id,
            &relative,
            archive_path,
            root_member,
            fields,
        )?;
        if !root_member {
            self.append_member(root_id, relative, fields.mode, content)?;
        }
        Ok(())
    }

    fn validate_member_parent(
        &self,
        root_id: CacheRootId,
        relative: &str,
        root_member: bool,
    ) -> Result<(), &'static str> {
        if !root_member && !self.has_parent_directories(root_id, relative) {
            return Err("qualification_cache_payload_parent_missing");
        }
        Ok(())
    }

    fn validate_root_member(
        &mut self,
        root_id: CacheRootId,
        root_member: bool,
        fields: HeaderFields,
    ) -> Result<(), &'static str> {
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
        } else if expected_file_root || !root.root_seen {
            return Err("qualification_cache_payload_root_mismatch");
        }
        Ok(())
    }

    fn append_member(
        &mut self,
        root_id: CacheRootId,
        relative: String,
        mode: u32,
        content: MemberContent,
    ) -> Result<(), &'static str> {
        self.add_inventory_text(relative.len())?;
        let root = self
            .roots
            .get_mut(&root_id)
            .ok_or("qualification_cache_payload_root_mismatch")?;
        root.entries.push(ArchiveEntry {
            path: relative,
            kind: match content.kind {
                MemberKind::Directory => ArchiveEntryKind::Directory,
                MemberKind::File => ArchiveEntryKind::File,
                MemberKind::Symlink => ArchiveEntryKind::Symlink,
            },
            mode,
            size: content.size,
            content_digest: content.digest,
            hardlink_target: content.hardlink_target,
            link_target: content.link_target,
            stat: None,
        });
        Ok(())
    }

    fn add_inventory_text(&mut self, amount: usize) -> Result<(), &'static str> {
        self.inventory_text_bytes = self
            .inventory_text_bytes
            .checked_add(amount)
            .filter(|size| *size <= MAX_INVENTORY_TEXT_BYTES)
            .ok_or("qualification_cache_payload_limit")?;
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
                .directories
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
        roots: &CacheRootSet,
    ) -> Result<PayloadManifest, &'static str> {
        if self.roots.len() != roots.roots.len() {
            return Err("qualification_cache_payload_root_mismatch");
        }
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
        }
        if super::super::useful_state_digest(&manifest.roots)? != manifest.state_digest {
            return Err("qualification_cache_payload_state_mismatch");
        }
        Ok(manifest)
    }
}
