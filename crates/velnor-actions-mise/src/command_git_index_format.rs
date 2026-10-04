//! Complete layout and checksum admission before native Git index reads.
use std::io::{Error, ErrorKind, Result};
#[path = "command_git_index_format_path.rs"]
mod path;
use path::{read_nul_range, read_ofs, valid_name_length, valid_path, valid_path_range};

const MAX_INPUT: usize = 64 * 1024 * 1024;
const HEADER_LEN: usize = 12;
const STAT_LEN: usize = 40;
#[derive(Debug, PartialEq, Eq)]
enum ParseFailure {
    Invalid,
    Split,
}
#[derive(Debug)]
struct IndexParser<'a> {
    data: &'a [u8],
    limit: usize,
    version: u32,
    oid_width: usize,
    previous_path: Vec<u8>,
    v4_prefix_lengths: Vec<usize>,
    semantic_invalid: bool,
}
/// Validate one complete, non-split Git index in versions 2, 3, or 4.
pub(super) fn validate(data: &[u8]) -> Result<usize> {
    if data.len() > MAX_INPUT {
        return Err(invalid("git_index_too_large"));
    }
    let checksums = super::checksum::Checksums::calculate(data);
    let sha1 = parse_width(data, 20);
    let sha256 = parse_width(data, 32);
    match (sha1, sha256) {
        (Ok(()), Ok(())) => Err(invalid("git_index_oid_width_ambiguous")),
        (Ok(()) | Err(ParseFailure::Invalid | ParseFailure::Split), Err(ParseFailure::Split))
        | (Err(ParseFailure::Split), Ok(()) | Err(ParseFailure::Invalid)) => {
            Err(invalid("git_index_split_unsupported"))
        }
        (Ok(()), Err(ParseFailure::Invalid)) if checksums.sha1_matches() => Ok(20),
        (Err(ParseFailure::Invalid), Ok(())) if checksums.sha256_matches() => Ok(32),
        (Ok(()), Err(ParseFailure::Invalid)) | (Err(ParseFailure::Invalid), Ok(())) => {
            Err(invalid("git_index_checksum_mismatch"))
        }
        (Err(_), Err(_)) => Err(invalid("git_index_invalid")),
    }
}
fn invalid(code: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, code)
}
fn parse_width(data: &[u8], oid_width: usize) -> std::result::Result<(), ParseFailure> {
    let minimum = HEADER_LEN
        .checked_add(oid_width)
        .ok_or(ParseFailure::Invalid)?;
    if data.len() < minimum {
        return Err(ParseFailure::Invalid);
    }
    let mut cursor: usize = 0;
    if data.get(..4) != Some(b"DIRC") {
        return Err(ParseFailure::Invalid);
    }
    cursor = cursor.checked_add(4).ok_or(ParseFailure::Invalid)?;
    let version = read_u32(data, &mut cursor).ok_or(ParseFailure::Invalid)?;
    if !matches!(version, 2..=4) {
        return Err(ParseFailure::Invalid);
    }
    let count = read_u32(data, &mut cursor).ok_or(ParseFailure::Invalid)?;
    let count = usize::try_from(count).map_err(|_| ParseFailure::Invalid)?;
    let checksum_start = data
        .len()
        .checked_sub(oid_width)
        .ok_or(ParseFailure::Invalid)?;
    if checksum_start < cursor {
        return Err(ParseFailure::Invalid);
    }
    let minimum_entry = STAT_LEN
        .checked_add(oid_width)
        .and_then(|length| length.checked_add(2))
        .ok_or(ParseFailure::Invalid)?;
    let available = checksum_start - cursor;
    if count > available / minimum_entry {
        return Err(ParseFailure::Invalid);
    }
    let mut parser = IndexParser {
        data,
        limit: checksum_start,
        version,
        oid_width,
        previous_path: Vec::new(),
        v4_prefix_lengths: Vec::new(),
        semantic_invalid: false,
    };
    let mut entry_starts = Vec::with_capacity(count);
    for _ in 0..count {
        let entry_start = cursor;
        entry_starts.push(entry_start);
        cursor = parser.parse_entry(cursor, entry_start)?;
    }
    let entry_end = cursor;
    parser.parse_extensions(entry_end, count, &entry_starts)?;
    if parser.semantic_invalid {
        Err(ParseFailure::Invalid)
    } else {
        Ok(())
    }
}
impl IndexParser<'_> {
    fn parse_entry(
        &mut self,
        mut cursor: usize,
        entry_start: usize,
    ) -> std::result::Result<usize, ParseFailure> {
        let (next, name_length) = self.read_entry_prefix(cursor)?;
        cursor = next;
        if self.version == 4 {
            self.parse_v4_path(&mut cursor, name_length)?;
            return Ok(cursor);
        }
        let (path_start, path_end, nul_ok) = self.read_v23_path(&mut cursor, name_length)?;
        self.semantic_invalid |=
            !nul_ok || !valid_path_range(self.data, path_start, path_end, name_length);
        self.finish_v23(cursor, entry_start)
    }
    fn read_entry_prefix(
        &mut self,
        cursor: usize,
    ) -> std::result::Result<(usize, usize), ParseFailure> {
        let fixed = STAT_LEN
            .checked_add(self.oid_width)
            .and_then(|length| length.checked_add(2))
            .ok_or(ParseFailure::Invalid)?;
        let fixed_end = cursor.checked_add(fixed).ok_or(ParseFailure::Invalid)?;
        if fixed_end > self.limit {
            return Err(ParseFailure::Invalid);
        }
        // Gitlinks can start repository commands in a submodule. Discovery's
        // private authority currently admits only ordinary files and symlinks.
        let mode = read_u32_at(self.data, cursor + 24).ok_or(ParseFailure::Invalid)?;
        self.semantic_invalid |= !matches!(mode, 0o100_644 | 0o100_755 | 0o120_000);
        let flags = read_u16(self.data, fixed_end - 2).ok_or(ParseFailure::Invalid)?;
        let name_length = usize::from(flags & 0x0fff);
        if flags & 0x4000 == 0 {
            return Ok((fixed_end, name_length));
        }
        self.semantic_invalid |= self.version == 2;
        let extra_end = fixed_end.checked_add(2).ok_or(ParseFailure::Invalid)?;
        if extra_end > self.limit {
            return Err(ParseFailure::Invalid);
        }
        let extra = read_u16(self.data, fixed_end).ok_or(ParseFailure::Invalid)?;
        self.semantic_invalid |= extra & 0x9fff != 0;
        Ok((extra_end, name_length))
    }
    fn read_v23_path(
        &self,
        cursor: &mut usize,
        name_length: usize,
    ) -> std::result::Result<(usize, usize, bool), ParseFailure> {
        let path_start = *cursor;
        if name_length == 0x0fff {
            let (path_end, nul_end) =
                read_nul_range(self.data, *cursor, self.limit).ok_or(ParseFailure::Invalid)?;
            *cursor = nul_end;
            return Ok((path_start, path_end, true));
        }
        let path_end = (*cursor)
            .checked_add(name_length)
            .ok_or(ParseFailure::Invalid)?;
        let nul_end = path_end.checked_add(1).ok_or(ParseFailure::Invalid)?;
        if nul_end > self.limit {
            return Err(ParseFailure::Invalid);
        }
        let nul_ok = self.data.get(path_end) == Some(&0);
        *cursor = nul_end;
        Ok((path_start, path_end, nul_ok))
    }
    fn parse_v4_path(
        &mut self,
        cursor: &mut usize,
        name_length: usize,
    ) -> std::result::Result<(), ParseFailure> {
        let strip = read_ofs(self.data, cursor, self.limit).ok_or(ParseFailure::Invalid)?;
        let mut prefix = if self.previous_path.is_empty() {
            0
        } else {
            if let Some(prefix) = self.previous_path.len().checked_sub(strip) {
                prefix
            } else {
                self.semantic_invalid = true;
                0
            }
        };
        let path_start = *cursor;
        // Native Git advances by the encoded full name length, except for
        // CE_NAMEMASK. Following the first NUL for every entry could hide a
        // native-visible extension behind a malformed compressed name.
        let suffix_length = if name_length == 0x0fff {
            0x0fff
        } else {
            if let Some(length) = name_length.checked_sub(prefix) {
                length
            } else {
                self.semantic_invalid = true;
                prefix = 0;
                name_length
            }
        };
        self.v4_prefix_lengths.push(prefix);
        let (_, suffix_end, nul_ok) = self.read_v23_path(cursor, suffix_length)?;
        let suffix = self
            .data
            .get(path_start..suffix_end)
            .ok_or(ParseFailure::Invalid)?;
        self.previous_path.truncate(prefix);
        self.previous_path.extend_from_slice(suffix);
        self.semantic_invalid |= !nul_ok
            || !valid_name_length(name_length, self.previous_path.len())
            || !valid_path(&self.previous_path);
        Ok(())
    }
    fn finish_v23(
        &mut self,
        cursor: usize,
        entry_start: usize,
    ) -> std::result::Result<usize, ParseFailure> {
        let padding = (8 - ((cursor - entry_start) % 8)) % 8;
        let padded_end = cursor.checked_add(padding).ok_or(ParseFailure::Invalid)?;
        if padded_end > self.limit {
            return Err(ParseFailure::Invalid);
        }
        self.semantic_invalid |= self
            .data
            .get(cursor..padded_end)
            .is_none_or(|padding| padding.iter().any(|byte| *byte != 0));
        Ok(padded_end)
    }
    fn parse_extensions(
        &mut self,
        entry_end: usize,
        entry_count: usize,
        entry_starts: &[usize],
    ) -> std::result::Result<(), ParseFailure> {
        let mut cursor = entry_end;
        let mut saw_eoie = false;
        while cursor < self.limit {
            let header_end = cursor.checked_add(8).ok_or(ParseFailure::Invalid)?;
            if header_end > self.limit {
                return Err(ParseFailure::Invalid);
            }
            let signature = self
                .data
                .get(cursor..cursor + 4)
                .ok_or(ParseFailure::Invalid)?;
            if signature == b"link" {
                return Err(ParseFailure::Split);
            }
            self.semantic_invalid |= !signature.first().is_some_and(u8::is_ascii_uppercase);
            let size = read_u32_at(self.data, cursor + 4).ok_or(ParseFailure::Invalid)?;
            let payload_start = header_end;
            let payload_end = payload_start
                .checked_add(usize::try_from(size).map_err(|_| ParseFailure::Invalid)?)
                .ok_or(ParseFailure::Invalid)?;
            if payload_end > self.limit {
                return Err(ParseFailure::Invalid);
            }
            self.semantic_invalid |= saw_eoie;
            let payload = self
                .data
                .get(payload_start..payload_end)
                .ok_or(ParseFailure::Invalid)?;
            match signature {
                b"EOIE" => {
                    self.semantic_invalid |= !validate_eoie(payload, self.oid_width, entry_end);
                    saw_eoie = true;
                }
                b"IEOT" => {
                    // V4 block resets have a second native cursor authority.
                    // Refuse them until that layout can be admitted completely.
                    self.semantic_invalid |= self.version == 4
                        || !validate_ieot(
                            payload,
                            entry_count,
                            entry_starts,
                            &self.v4_prefix_lengths,
                        );
                }
                _ => {}
            }
            cursor = payload_end;
        }
        if cursor != self.limit {
            return Err(ParseFailure::Invalid);
        }
        Ok(())
    }
}
fn validate_eoie(payload: &[u8], oid_width: usize, entry_end: usize) -> bool {
    let Some(expected) = 4usize.checked_add(oid_width) else {
        return false;
    };
    if payload.len() != expected {
        return false;
    }
    let mut cursor = 0;
    let Some(end) = read_u32(payload, &mut cursor).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    end == entry_end
}
fn validate_ieot(
    payload: &[u8],
    entry_count: usize,
    entry_starts: &[usize],
    v4_prefix_lengths: &[usize],
) -> bool {
    if payload.len() < 12 {
        return false;
    }
    let mut cursor = 0;
    if read_u32(payload, &mut cursor) != Some(1) || !(payload.len() - cursor).is_multiple_of(8) {
        return false;
    }
    let mut total = 0usize;
    while cursor < payload.len() {
        if total >= entry_count {
            return false;
        }
        let Some(offset) =
            read_u32(payload, &mut cursor).and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        if entry_starts.get(total).copied() != Some(offset) {
            return false;
        }
        if !v4_prefix_lengths.is_empty() && v4_prefix_lengths.get(total) != Some(&0) {
            return false;
        }
        let Some(block_count) =
            read_u32(payload, &mut cursor).and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        if block_count == 0 {
            return false;
        }
        let Some(next) = total.checked_add(block_count) else {
            return false;
        };
        if next > entry_count {
            return false;
        }
        total = next;
    }
    total == entry_count
}
fn read_u32(data: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let bytes = data.get(*cursor..end)?;
    *cursor = end;
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}
fn read_u16(data: &[u8], cursor: usize) -> Option<u16> {
    let end = cursor.checked_add(2)?;
    Some(u16::from_be_bytes(data.get(cursor..end)?.try_into().ok()?))
}

fn read_u32_at(data: &[u8], cursor: usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    Some(u32::from_be_bytes(data.get(cursor..end)?.try_into().ok()?))
}

#[cfg(test)]
#[path = "command_git_index_format_tests.rs"]
mod tests;
