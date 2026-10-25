use crate::error::HostError;

pub const MAX_FRAME: usize = 1024 * 1024;

pub const SOCKET_DIR_MODE: u32 = 0o700;

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
