use crate::error::HostError;

use super::valid_timestamp;

#[derive(Clone, Copy)]
pub(super) enum ArchiveDirectory {
    Root,
    Pages,
    Blocks,
}

pub(super) fn directory_kind(path: &[u8]) -> Result<ArchiveDirectory, HostError> {
    match path {
        b"." | b"./" | b"_diag" | b"_diag/" => Ok(ArchiveDirectory::Root),
        _ => match relative_archive_path(path)
            .strip_suffix(b"/")
            .unwrap_or(relative_archive_path(path))
        {
            b"pages" => Ok(ArchiveDirectory::Pages),
            b"blocks" => Ok(ArchiveDirectory::Blocks),
            _ => Err(HostError::Ownership),
        },
    }
}

pub(super) fn relative_archive_path(path: &[u8]) -> &[u8] {
    path.strip_prefix(b"./")
        .or_else(|| path.strip_prefix(b"_diag/"))
        .unwrap_or(path)
}

pub(super) fn auxiliary_file_name(path: &[u8]) -> Result<bool, HostError> {
    if let Some(name) = path.strip_prefix(b"SelfUpdate-") {
        return valid_self_update_name(name)
            .then_some(true)
            .ok_or(HostError::Ownership);
    }
    for prefix in [
        b"GitCheck_".as_slice(),
        b"ActionsCheck_",
        b"NodeJsCheck_",
        b"InternetCheck_",
    ] {
        if let Some(name) = path.strip_prefix(prefix) {
            return valid_runner_check_name(name)
                .then_some(true)
                .ok_or(HostError::Ownership);
        }
    }
    let Some((directory, name)) = split_once_byte(path, b'/') else {
        return Ok(false);
    };
    let valid = match directory {
        b"pages" => valid_page_name(name),
        b"blocks" => valid_block_name(name),
        _ => return Ok(false),
    };
    valid.then_some(true).ok_or(HostError::Ownership)
}

fn valid_self_update_name(name: &[u8]) -> bool {
    let Some(timestamp) = name.strip_suffix(b".log") else {
        return false;
    };
    valid_compact_timestamp(timestamp)
}

fn valid_runner_check_name(name: &[u8]) -> bool {
    let Some(timestamp) = name.strip_suffix(b"-utc.log") else {
        return false;
    };
    valid_compact_timestamp(timestamp)
}

fn valid_compact_timestamp(timestamp: &[u8]) -> bool {
    if timestamp.len() != 15
        || timestamp[8] != b'-'
        || !timestamp
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
    {
        return false;
    }
    let date = format!(
        "{}-{}-{} {}:{}:{}Z",
        std::str::from_utf8(&timestamp[0..4]).unwrap_or_default(),
        std::str::from_utf8(&timestamp[4..6]).unwrap_or_default(),
        std::str::from_utf8(&timestamp[6..8]).unwrap_or_default(),
        std::str::from_utf8(&timestamp[9..11]).unwrap_or_default(),
        std::str::from_utf8(&timestamp[11..13]).unwrap_or_default(),
        std::str::from_utf8(&timestamp[13..15]).unwrap_or_default(),
    );
    valid_timestamp(date.as_bytes())
}

fn valid_page_name(name: &[u8]) -> bool {
    let Some(stem) = name.strip_suffix(b".log") else {
        return false;
    };
    let Some((timeline, remainder)) = split_once_byte(stem, b'_') else {
        return false;
    };
    let Some((record, page)) = split_once_byte(remainder, b'_') else {
        return false;
    };
    valid_guid(timeline) && valid_guid(record) && positive_decimal(page)
}

fn valid_block_name(name: &[u8]) -> bool {
    let Some((identities, block)) = split_once_byte(name, b'.') else {
        return false;
    };
    let Some((timeline, record)) = split_once_byte(identities, b'_') else {
        return false;
    };
    valid_guid(timeline) && valid_guid(record) && positive_decimal(block)
}

fn split_once_byte(value: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    let index = value.iter().position(|byte| *byte == separator)?;
    Some((&value[..index], &value[index + 1..]))
}

fn valid_guid(value: &[u8]) -> bool {
    value.len() == 36
        && value.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn positive_decimal(value: &[u8]) -> bool {
    !value.is_empty()
        && value.iter().all(u8::is_ascii_digit)
        && std::str::from_utf8(value)
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
            .is_some_and(|number| number > 0)
}
