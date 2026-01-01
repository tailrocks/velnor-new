use std::io::Read;

use super::{BLOCK_SIZE, BLOCK_SIZE_U64};

pub(super) fn read_metadata<R: Read>(reader: &mut R, size: u64) -> Result<Vec<u8>, String> {
    let length = usize::try_from(size).map_err(|_| "metadata size conversion failed".to_owned())?;
    let mut body = vec![0_u8; length];
    read_exact(reader, &mut body, "truncated tar metadata")?;
    let padding = (BLOCK_SIZE_U64 - size % BLOCK_SIZE_U64) % BLOCK_SIZE_U64;
    let padding_length = usize::try_from(padding)
        .map_err(|_| "tar metadata padding length conversion failed".to_owned())?;
    let mut padding_bytes = [0_u8; BLOCK_SIZE];
    read_exact(
        reader,
        &mut padding_bytes[..padding_length],
        "truncated tar metadata padding",
    )?;
    if padding_bytes[..padding_length]
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err("nonzero tar metadata padding".to_owned());
    }
    Ok(body)
}

pub(super) fn skip_padded<R: Read>(reader: &mut R, size: u64) -> Result<(), String> {
    let padded = size
        .checked_add(BLOCK_SIZE_U64 - 1)
        .ok_or_else(|| "tar member size overflow".to_owned())?
        / BLOCK_SIZE_U64
        * BLOCK_SIZE_U64;
    skip_exact(reader, padded, "truncated tar member")
}

fn skip_exact<R: Read>(reader: &mut R, mut amount: u64, error: &str) -> Result<(), String> {
    let mut buffer = [0_u8; 16 * 1024];
    while amount > 0 {
        let buffer_len = u64::try_from(buffer.len())
            .map_err(|_| "tar skip length conversion failed".to_owned())?;
        let cap = usize::try_from(amount.min(buffer_len))
            .map_err(|_| "tar skip length conversion failed".to_owned())?;
        let read = reader
            .read(&mut buffer[..cap])
            .map_err(|problem| format!("archive read failed: {problem}"))?;
        if read == 0 {
            return Err(error.to_owned());
        }
        amount -= u64::try_from(read).map_err(|_| "tar skip count conversion failed".to_owned())?;
    }
    Ok(())
}

pub(super) fn drain_zero_tail<R: Read>(reader: &mut R) -> Result<(), String> {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|problem| format!("archive read failed: {problem}"))?;
        if read == 0 {
            return Ok(());
        }
        if buffer[..read].iter().any(|byte| *byte != 0) {
            return Err("nonzero bytes follow tar end markers".to_owned());
        }
    }
}

pub(super) fn read_exact<R: Read>(
    reader: &mut R,
    target: &mut [u8],
    error: &str,
) -> Result<(), String> {
    reader
        .read_exact(target)
        .map_err(|problem| format!("{error}: {problem}"))
}
