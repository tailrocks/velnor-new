//! Length-prefixed local frames. The socket directory mode is `0700`.

use crate::HostError;

/// Maximum payload size: 1 MiB.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Directory mode for the private socket directory.
pub const SOCKET_DIR_MODE: u32 = 0o700;

/// Encode a frame. The length prefix is big-endian `u32`.
///
/// # Errors
///
/// Returns [`HostError::Frame`] when the payload exceeds [`MAX_FRAME`].
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, HostError> {
    let len = u32::try_from(payload.len()).map_err(|_| HostError::Frame)?;
    if usize::try_from(len).map_err(|_| HostError::Frame)? > MAX_FRAME {
        return Err(HostError::Frame);
    }
    let mut out = Vec::with_capacity(payload.len().saturating_add(4));
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Decode one frame from `bytes`.
///
/// # Errors
///
/// Returns [`HostError::Frame`] when the prefix is short, oversized, or truncated.
pub fn decode_frame(bytes: &[u8]) -> Result<&[u8], HostError> {
    if bytes.len() < 4 {
        return Err(HostError::Frame);
    }
    let mut prefix = [0_u8; 4];
    prefix.copy_from_slice(&bytes[..4]);
    let len = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| HostError::Frame)?;
    if len > MAX_FRAME || bytes.len() < len.saturating_add(4) {
        return Err(HostError::Frame);
    }
    Ok(&bytes[4..len.saturating_add(4)])
}

#[cfg(test)]
mod tests;
