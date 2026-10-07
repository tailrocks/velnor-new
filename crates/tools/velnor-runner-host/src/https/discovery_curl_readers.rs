use std::io::Read;

use zeroize::Zeroize;

use super::STATUS_MARKER;

pub(super) enum BodyReadError {
    TooLarge,
    Io,
}

pub(super) fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, BodyReadError> {
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    let mut chunk = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => return Ok(body),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                body.zeroize();
                return Err(BodyReadError::Io);
            }
        };
        if body.len().checked_add(read).is_none_or(|size| size > limit) {
            body.zeroize();
            return Err(BodyReadError::TooLarge);
        }
        body.extend_from_slice(&chunk[..read]);
    }
}

pub(super) fn read_tail(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, BodyReadError> {
    let mut tail = Vec::with_capacity(limit);
    let mut chunk = [0_u8; 512];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => return Ok(tail),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                tail.zeroize();
                return Err(BodyReadError::Io);
            }
        };
        if read >= limit {
            tail.clear();
            tail.extend_from_slice(&chunk[read - limit..read]);
        } else {
            let overflow = tail.len().saturating_add(read).saturating_sub(limit);
            if overflow > 0 {
                tail.drain(..overflow);
            }
            tail.extend_from_slice(&chunk[..read]);
        }
    }
}

pub(super) fn parse_http_status(tail: &[u8]) -> Option<u16> {
    let tail = std::str::from_utf8(tail).ok()?.trim();
    let status = tail.strip_prefix(STATUS_MARKER)?.parse::<u16>().ok()?;
    (100..600).contains(&status).then_some(status)
}
