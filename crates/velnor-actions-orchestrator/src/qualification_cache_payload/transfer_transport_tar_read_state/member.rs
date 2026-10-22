use super::super::{
    FILE_DOMAIN, effective_value, hash_entry, hex, root_name, utf8_text, validate_archive_path,
};
use super::payload;
use super::{CacheRootId, FileIdentity, HeaderFields, MemberContent, MemberKind, ParseState};
use std::io::Read;
use tar::Entry;

pub(super) fn read_member_content<R: Read>(
    state: &mut ParseState<'_>,
    entry: &mut Entry<'_, R>,
    root_id: CacheRootId,
    relative: &str,
    archive_path: &str,
    root_member: bool,
    fields: HeaderFields,
) -> Result<MemberContent, &'static str> {
    match fields.kind {
        kind if kind.is_file() => {
            read_file_member(state, entry, root_id, relative, root_member, fields)
        }
        kind if kind.is_dir() => {
            if fields.size != 0 {
                return Err("qualification_cache_payload_entry_invalid");
            }
            if !root_member {
                state.directories.insert(archive_path.to_owned());
            }
            Ok(MemberContent {
                kind: MemberKind::Directory,
                size: 0,
                digest: [0; 32],
                link_target: None,
                hardlink_target: None,
            })
        }
        kind if kind.is_symlink() => {
            read_symlink_member(state, entry, relative, root_member, fields)
        }
        kind if kind.is_hard_link() => {
            read_hardlink_member(state, entry, root_id, relative, root_member, fields)
        }
        _ => Err("qualification_cache_payload_entry_unsupported"),
    }
}

fn read_file_member<R: Read>(
    state: &mut ParseState<'_>,
    entry: &mut Entry<'_, R>,
    root_id: CacheRootId,
    relative: &str,
    root_member: bool,
    fields: HeaderFields,
) -> Result<MemberContent, &'static str> {
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
        let root = state
            .roots
            .get(&root_id)
            .ok_or("qualification_cache_payload_root_mismatch")?;
        let expected = format!("b3-{}", hex(&digest));
        if root.manifest.digest.as_deref() != Some(expected.as_str()) {
            return Err("qualification_cache_payload_digest_mismatch");
        }
    } else {
        state.previous_files.insert(
            (root_id, relative.to_owned()),
            FileIdentity {
                mode: fields.mode,
                size: fields.size,
                digest,
                hardlink_target: None,
            },
        );
    }
    Ok(MemberContent {
        kind: MemberKind::File,
        size: read_size,
        digest,
        link_target: None,
        hardlink_target: None,
    })
}

fn read_symlink_member<R: Read>(
    state: &mut ParseState<'_>,
    entry: &mut Entry<'_, R>,
    relative: &str,
    root_member: bool,
    fields: HeaderFields,
) -> Result<MemberContent, &'static str> {
    if fields.size != 0 || root_member {
        return Err("qualification_cache_payload_entry_invalid");
    }
    let target = effective_value(entry, "linkpath")?;
    let target = utf8_text(target, "qualification_cache_payload_symlink_invalid")?;
    payload::validate_internal_link(relative, &target)?;
    state.add_inventory_text(target.len())?;
    let size = u64::try_from(target.len()).map_err(|_| "qualification_cache_payload_limit")?;
    let digest = *blake3::hash(target.as_bytes()).as_bytes();
    Ok(MemberContent {
        kind: MemberKind::Symlink,
        size,
        digest,
        link_target: Some(target),
        hardlink_target: None,
    })
}

fn read_hardlink_member<R: Read>(
    state: &mut ParseState<'_>,
    entry: &mut Entry<'_, R>,
    root_id: CacheRootId,
    relative: &str,
    root_member: bool,
    fields: HeaderFields,
) -> Result<MemberContent, &'static str> {
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
    state.add_inventory_text(target.len())?;
    let previous = state
        .previous_files
        .get(&(root_id, target.clone()))
        .filter(|file| file.hardlink_target.is_none())
        .ok_or("qualification_cache_payload_hardlink_invalid")?;
    if previous.mode != fields.mode {
        return Err("qualification_cache_payload_hardlink_invalid");
    }
    let identity = previous.clone();
    state.previous_files.insert(
        (root_id, relative.to_owned()),
        FileIdentity {
            hardlink_target: Some(target.clone()),
            ..identity.clone()
        },
    );
    Ok(MemberContent {
        kind: MemberKind::File,
        size: identity.size,
        digest: identity.digest,
        link_target: None,
        hardlink_target: Some(target),
    })
}
