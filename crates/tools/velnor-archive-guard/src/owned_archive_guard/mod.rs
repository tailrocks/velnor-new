//! Bounded structural preflight for raw candidate and gzip Cargo tar inputs.

use std::io::{self, Read};

use flate2::read::MultiGzDecoder;

#[path = "pax.rs"]
mod pax;
#[path = "profile.rs"]
mod profile;
#[path = "stream.rs"]
mod stream;
use pax::{check_header as check_pax_header, parse as parse_pax};
use profile::{ArchiveEncoding, Profile, profile};
use stream::{drain_zero_tail, read_exact, read_metadata, skip_padded};

const BLOCK_SIZE: usize = 512;
const BLOCK_SIZE_U64: u64 = 512;

struct Bounded<R> {
    inner: R,
    limit: u64,
    read: u64,
    label: &'static str,
}

impl<R> Bounded<R> {
    const fn new(inner: R, limit: u64, label: &'static str) -> Self {
        Self {
            inner,
            limit,
            read: 0,
            label,
        }
    }
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.read == self.limit {
            let mut probe = [0_u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} limit exceeded", self.label),
                )),
            };
        }
        let remaining = self.limit.saturating_sub(self.read);
        let buffer_len = u64::try_from(buffer.len())
            .map_err(|_| io::Error::other("bounded read length conversion failed"))?;
        let cap = usize::try_from(remaining.min(buffer_len))
            .map_err(|_| io::Error::other("bounded read length conversion failed"))?;
        let amount = self.inner.read(&mut buffer[..cap])?;
        let amount = u64::try_from(amount)
            .map_err(|_| io::Error::other("bounded read count conversion failed"))?;
        self.read = self.read.saturating_add(amount);
        usize::try_from(amount)
            .map_err(|_| io::Error::other("bounded read count conversion failed"))
    }
}

/// Check archive framing and resource limits before a higher-level tar parser runs.
///
/// `candidate` accepts raw tar from the release producer. `cargo-package`
/// accepts gzip-compressed tar. Each mode rejects the other encoding.
///
/// # Errors
///
/// Returns a diagnostic when the mode, archive framing, metadata, compression,
/// or resource bounds do not match the fixed policy.
pub fn validate<R: Read>(reader: R, mode: &str) -> Result<(), String> {
    validate_with_profile(reader, profile(mode)?)
}

fn validate_with_profile<R: Read>(reader: R, policy: Profile) -> Result<(), String> {
    match policy.encoding {
        ArchiveEncoding::RawTar => {
            let input = Bounded::new(reader, policy.input_limit, "tar input");
            let mut archive = Bounded::new(input, policy.archive_limit, "tar size");
            scan_tar(&mut archive, policy)
        }
        ArchiveEncoding::Gzip => {
            let input = Bounded::new(reader, policy.input_limit, "compressed input");
            let decoder = MultiGzDecoder::new(input);
            let mut archive = Bounded::new(decoder, policy.archive_limit, "gzip expansion");
            scan_tar(&mut archive, policy)
        }
    }
}

fn scan_tar<R: Read>(reader: &mut R, profile: Profile) -> Result<(), String> {
    let mut state = ScanState::default();
    loop {
        let header = read_header(reader)?;
        if header.iter().all(|byte| *byte == 0) {
            finish_tar(reader, state.pending_extension)?;
            break;
        }
        check_checksum(&header)?;
        let size = parse_octal(&header[124..136], "tar member size")?;
        if handle_extension(reader, profile, &header, size, &mut state)? {
            continue;
        }
        scan_member(
            reader,
            profile,
            header[156],
            &header[..100],
            size,
            &mut state,
        )?;
    }
    Ok(())
}

#[derive(Default)]
struct ScanState {
    members: usize,
    payload: u64,
    metadata: u64,
    pending_extension: bool,
}

fn read_header<R: Read>(reader: &mut R) -> Result<[u8; BLOCK_SIZE], String> {
    let mut header = [0_u8; BLOCK_SIZE];
    read_exact(reader, &mut header, "truncated tar header")?;
    Ok(header)
}

fn finish_tar<R: Read>(reader: &mut R, pending_extension: bool) -> Result<(), String> {
    if pending_extension {
        return Err("tar ends after an extension header".to_owned());
    }
    let mut second = [0_u8; BLOCK_SIZE];
    read_exact(reader, &mut second, "single tar end marker")?;
    if second.iter().any(|byte| *byte != 0) {
        return Err("tar end marker is not followed by zero block".to_owned());
    }
    drain_zero_tail(reader)
}

fn handle_extension<R: Read>(
    reader: &mut R,
    profile: Profile,
    header: &[u8; BLOCK_SIZE],
    size: u64,
    state: &mut ScanState,
) -> Result<bool, String> {
    match header[156] {
        b'x' => {
            check_pax_header(profile, size, &mut state.metadata)?;
            if state.pending_extension {
                return Err("multiple local tar extension headers".to_owned());
            }
            let body = read_metadata(reader, size)?;
            parse_pax(&body)?;
            state.pending_extension = true;
        }
        b'g' => return Err("global PAX tar extensions are forbidden".to_owned()),
        b'L' | b'K' => return Err("GNU long tar extension is forbidden".to_owned()),
        b'S' => return Err("GNU sparse tar members are forbidden".to_owned()),
        _ => return Ok(false),
    }
    Ok(true)
}

fn scan_member<R: Read>(
    reader: &mut R,
    profile: Profile,
    kind: u8,
    raw_name: &[u8],
    size: u64,
    state: &mut ScanState,
) -> Result<(), String> {
    let allowed = match kind {
        0 | b'0' => profile.allowed_types & 0b001 != 0,
        b'5' => profile.allowed_types & 0b010 != 0,
        b'2' => profile.allowed_types & 0b100 != 0,
        _ => false,
    };
    if !allowed {
        return Err("unsupported tar member type".to_owned());
    }
    let old_v7_directory = kind == 0 && raw_name_ends_with_slash(raw_name);
    if (kind == b'5' || kind == b'2' || old_v7_directory) && size != 0 {
        return Err("directory and symlink tar members must have zero size".to_owned());
    }
    if state.members >= profile.member_limit {
        return Err("tar member count limit exceeded".to_owned());
    }
    state.payload = state
        .payload
        .checked_add(size)
        .ok_or_else(|| "tar payload size overflow".to_owned())?;
    if state.payload > profile.payload_limit {
        return Err("tar payload limit exceeded".to_owned());
    }
    state.members += 1;
    state.pending_extension = false;
    skip_padded(reader, size)?;
    Ok(())
}

fn raw_name_ends_with_slash(name: &[u8]) -> bool {
    let mut end = 0;
    while end < name.len() && name[end] != 0 {
        end += 1;
    }
    end > 0 && name[end - 1] == b'/'
}

fn check_checksum(header: &[u8; BLOCK_SIZE]) -> Result<(), String> {
    let expected = parse_octal(&header[148..156], "tar header checksum")?;
    let unsigned = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum::<u64>();
    let signed = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                i64::from(b' ')
            } else {
                i64::from(i8::from_ne_bytes([*byte]))
            }
        })
        .sum::<i64>();
    if expected != unsigned && (u64::try_from(signed).ok() != Some(expected)) {
        return Err(format!(
            "tar header checksum mismatch: expected {expected}, computed {unsigned}"
        ));
    }
    Ok(())
}

fn parse_octal(field: &[u8], label: &str) -> Result<u64, String> {
    let mut value = 0_u64;
    let mut started = false;
    let mut ended = false;
    for byte in field {
        if *byte == 0 || *byte == b' ' {
            ended |= started;
            continue;
        }
        if ended || !(b'0'..=b'7').contains(byte) {
            return Err(format!("invalid octal {label}"));
        }
        started = true;
        value = value
            .checked_mul(8)
            .and_then(|current| current.checked_add(u64::from(*byte - b'0')))
            .ok_or_else(|| format!("{label} overflow"))?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
