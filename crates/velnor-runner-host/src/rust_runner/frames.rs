// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use zeroize::Zeroizing;

pub(super) const FRAME_LIMIT: usize = 64 * 1024;

fn push(bytes: &mut Zeroizing<Vec<u8>>, payload: &[u8]) -> Option<()> {
    if payload.is_empty() || payload.len() > FRAME_LIMIT {
        return None;
    }
    bytes.extend_from_slice(&u32::try_from(payload.len()).ok()?.to_be_bytes());
    bytes.extend_from_slice(payload);
    Some(())
}

pub(super) fn encode(jit: &str, trusted: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(8 + jit.len() + trusted.len()));
    push(&mut bytes, jit.as_bytes())?;
    push(&mut bytes, trusted)?;
    Some(bytes)
}

#[cfg(test)]
mod tests;
